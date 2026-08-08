import { appendFileSync } from 'node:fs';
import path from 'node:path';
import { execFileSync, type ExecFileSyncOptions } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const SEMVER_TAG_PATTERN = /^v(\d+)\.(\d+)\.(\d+)$/;
const BOT_NAME = 'github-actions[bot]';
const BOT_EMAIL = '41898282+github-actions[bot]@users.noreply.github.com';

type Version = {
  major: number;
  minor: number;
  patch: number;
};

type CreatedTag = {
  tag: string;
  commit: string;
};

type ReleasePlan = {
  createdTags: CreatedTag[];
  finalCommit: string;
  finalTag: string;
  summaryLines: string[];
};

type GithubOutputs = {
  createdReleases: CreatedTag[];
  finalCommit: string;
  finalTag: string;
  summaryLines: string[];
};

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const repoRoot = path.resolve(__dirname, '../..');
const dryRun = process.env.RELEASE_TAGS_DRY_RUN === '1';

function runGit(args: string[], options: ExecFileSyncOptions = {}) {
  const output = execFileSync('git', args, {
    cwd: repoRoot,
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
    ...options,
  });

  return typeof output === 'string' ? output.trim() : '';
}

function isSemverTag(tag: string) {
  return SEMVER_TAG_PATTERN.test(tag);
}

function parseTag(tag: string): Version {
  const match = tag.match(SEMVER_TAG_PATTERN);
  if (!match) {
    throw new Error(`Tag '${tag}' is not a valid semantic version`);
  }

  return {
    major: Number(match[1]),
    minor: Number(match[2]),
    patch: Number(match[3]),
  };
}

function nextVersionForSubject(version: Version, subject: string): Version {
  if (/^feat(\([^)]+\))?!:/.test(subject)) {
    return {
      major: version.major + 1,
      minor: 0,
      patch: 0,
    };
  }

  if (/^feat(\([^)]+\))?:/.test(subject)) {
    return {
      major: version.major,
      minor: version.minor + 1,
      patch: 0,
    };
  }

  return {
    major: version.major,
    minor: version.minor,
    patch: version.patch + 1,
  };
}

function formatTag(version: Version) {
  return `v${version.major}.${version.minor}.${version.patch}`;
}

function getPointingSemverTag(commit: string) {
  const tags = runGit(['tag', '--points-at', commit]);
  if (!tags) {
    return '';
  }

  return tags
    .split('\n')
    .map((tag) => tag.trim())
    .filter(Boolean)
    .filter(isSemverTag)
    .sort((left, right) => left.localeCompare(right, undefined, { numeric: true }))
    .at(-1) ?? '';
}

function ensureGitIdentity() {
  runGit(['config', 'user.name', BOT_NAME]);
  runGit(['config', 'user.email', BOT_EMAIL]);
}

function getTaggedCommit(tag: string) {
  try {
    return runGit(['rev-list', '-n', '1', tag]);
  } catch {
    return '';
  }
}

function collectReleasePlan(commits: string[]): ReleasePlan {
  let version = { major: 0, minor: 0, patch: 0 };
  let haveVersion = false;
  let finalTag = '';
  let finalCommit = '';
  const summaryLines = [];

  for (const commit of commits) {
    const existingTag = getPointingSemverTag(commit);

    if (existingTag) {
      version = parseTag(existingTag);
      haveVersion = true;
      finalTag = existingTag;
      finalCommit = commit;
      summaryLines.push(`${existingTag} ${commit} existing`);
      continue;
    }

    if (!haveVersion) {
      haveVersion = true;
    }

    const subject = runGit(['log', '-1', '--format=%s', commit]);
    version = nextVersionForSubject(version, subject);
    const nextTag = formatTag(version);
    const taggedCommit = getTaggedCommit(nextTag);

    if (taggedCommit && taggedCommit !== commit) {
      throw new Error(
        `Computed tag ${nextTag} for ${commit}, but that tag already points to ${taggedCommit}`,
      );
    }

    finalTag = nextTag;
    finalCommit = commit;
    summaryLines.push(`${nextTag} ${commit} ${subject}`);
  }

  const createdTags = [];

  if (finalTag) {
    const taggedCommit = getTaggedCommit(finalTag);

    if (taggedCommit && taggedCommit !== finalCommit) {
      throw new Error(
        `Computed final tag ${finalTag} for ${finalCommit}, but that tag already points to ${taggedCommit}`,
      );
    }

    if (!taggedCommit) {
      createdTags.push({ tag: finalTag, commit: finalCommit });
    }
  }

  return {
    createdTags,
    finalCommit,
    finalTag,
    summaryLines,
  };
}

function publishTags(createdTags: CreatedTag[]) {
  if (createdTags.length === 0 || dryRun) {
    return;
  }

  ensureGitIdentity();

  for (const createdTag of createdTags) {
    runGit(['tag', '-a', createdTag.tag, createdTag.commit, '-m', `Release ${createdTag.tag}`]);
  }

  runGit(['push', 'origin', ...createdTags.map((createdTag) => `refs/tags/${createdTag.tag}`)]);
}

function writeGithubOutputs(outputs: GithubOutputs) {
  const outputPath = process.env.GITHUB_OUTPUT;
  const lines = [
    `created_releases=${JSON.stringify(outputs.createdReleases)}`,
    `created_release_count=${outputs.createdReleases.length}`,
    `final_tag=${outputs.finalTag}`,
    `final_commit=${outputs.finalCommit}`,
    'release_plan<<EOF',
    ...outputs.summaryLines,
    'EOF',
  ];

  if (outputPath) {
    appendFileSync(outputPath, `${lines.join('\n')}\n`, 'utf8');
    return;
  }

  process.stdout.write(`${lines.join('\n')}\n`);
}

function main() {
  runGit(['fetch', '--force', '--tags', 'origin', '+refs/heads/main:refs/remotes/origin/main']);

  const commitsOutput = runGit(['rev-list', '--first-parent', '--reverse', 'origin/main']);
  const commits = commitsOutput ? commitsOutput.split('\n').filter(Boolean) : [];

  if (commits.length === 0) {
    throw new Error('No commits found on origin/main');
  }

  const { createdTags, finalCommit, finalTag, summaryLines } = collectReleasePlan(commits);

  if (!finalTag || finalCommit !== commits.at(-1)) {
    throw new Error('Failed to derive a release tag for HEAD on main');
  }

  publishTags(createdTags);

  writeGithubOutputs({
    createdReleases: createdTags,
    finalTag,
    finalCommit,
    summaryLines,
  });
}

try {
  main();
} catch (error) {
  const message = error instanceof Error ? error.message : String(error);
  process.stderr.write(`${message}\n`);
  process.exit(1);
}
