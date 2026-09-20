// actions/github-script supplies the authenticated client; tests inject API doubles.
module.exports = async function prepareDraftRelease({ github, context, core, releaseId }) {
  // Dispatch candidates must never read or mutate release state.
  if (context.eventName !== 'push') return;
  if (!context.ref?.startsWith('refs/tags/')) throw new Error('Release preparation requires a tag push');

  const tag = context.ref.slice('refs/tags/'.length);
  const expected = context.sha;
  const repo = context.repo;
  let operation = 'resolve tag';
  // Never log Octokit error messages/requests: they can contain credentials.
  const diagnostic = (error) => {
    const status = Number.isInteger(error?.status) ? error.status : 'unknown';
    const requestId = String(error?.response?.headers?.['x-github-request-id'] || 'unavailable')
      .replace(/[^a-zA-Z0-9:-]/g, '').slice(0, 128);
    return `HTTP ${status}; request ID ${requestId}`;
  };
  const fail = (message) => {
    const error = new Error(message);
    error.releaseValidation = true;
    throw error;
  };
  const validateTag = async () => {
    operation = 'resolve tag';
    let object = (await github.rest.git.getRef({ ...repo, ref: `tags/${tag}` })).data.object;
    const seen = new Set();
    while (object.type === 'tag') {
      if (seen.has(object.sha) || seen.size >= 16) fail('Tag annotation chain is cyclic or exceeds 16 objects');
      seen.add(object.sha);
      object = (await github.rest.git.getTag({ ...repo, tag_sha: object.sha })).data.object;
    }
    if (object.type !== 'commit') fail(`Tag target must be a commit; observed ${object.type}`);
    if (object.sha !== expected) fail(`Tag moved: expected ${expected}, observed ${object.sha}`);
  };
  const findDraft = async () => {
    operation = 'list releases';
    const releases = await github.paginate(github.rest.repos.listReleases, { ...repo, per_page: 100 });
    const matches = releases.filter((release) => release.tag_name === tag);
    if (matches.some((release) => !release.draft)) fail('The matching release is already published; refusing to modify it');
    if (matches.length > 1) fail('Multiple matching draft releases exist; resolve the ambiguity before rerunning');
    return matches[0];
  };
  const complete = (release) => {
    if (!release.draft || release.tag_name !== tag || !Number.isSafeInteger(release.id) || release.id <= 0) {
      fail('GitHub did not return a valid matching draft release ID');
    }
    core.setOutput('release-id', String(release.id));
    core.info(`Prepared draft release ${release.id} for ${tag} at ${expected}`);
    return release.id;
  };

  try {
    await validateTag();
    // Failed-job reruns retain the preparation output. Validate its live state
    // without listing or creating releases before each platform packages/uploads.
    if (releaseId !== undefined) {
      operation = 'validate prepared draft release';
      const id = Number(releaseId);
      if (!/^\d+$/.test(String(releaseId)) || !Number.isSafeInteger(id) || id <= 0) {
        fail('Prepared release ID is missing or invalid');
      }
      const release = (await github.rest.repos.getRelease({ ...repo, release_id: id })).data;
      if (release.id !== id) fail('GitHub returned a different release ID');
      if (!release.draft) fail('The prepared release is already published; refusing to modify it');
      await validateTag();
      return complete(release);
    }
    const existing = await findDraft();
    if (existing) {
      await validateTag();
      return complete(existing);
    }
    // Recheck immediately before creation: the API must use an existing tag.
    await validateTag();
    operation = 'create draft release';
    let created;
    try {
      created = (await github.rest.repos.createRelease({
        ...repo, tag_name: tag, target_commitish: expected,
        name: `HifiMule ${tag}`, body: 'See the release notes for details.',
        draft: true, prerelease: false,
      })).data;
    } catch (creationError) {
      // A response can fail after creation, or another actor may have created it.
      // Reconcile once, without retrying a denied write.
      try {
        const recovered = await findDraft();
        if (recovered) {
          await validateTag();
          return complete(recovered);
        }
      } catch (recoveryError) {
        core.warning(`Draft reconciliation failed (${diagnostic(recoveryError)}); retaining the original creation failure`);
      }
      operation = 'create draft release';
      throw creationError;
    }
    await validateTag();
    return complete(created);
  } catch (error) {
    const detail = error.releaseValidation ? error.message : diagnostic(error);
    throw new Error(`Cannot ${operation} for ${tag} at ${expected}: ${detail}. Verify the remote tag and GITHUB_TOKEN contents: write authorization before rerunning.`);
  }
};
