import assert from 'node:assert/strict';
import test from 'node:test';
import prepareDraftRelease from '../prepare-draft-release.cjs';

const sha = 'a'.repeat(40);
const tag = 'v0.15.0';
const draft = { id: 42, tag_name: tag, draft: true, body: 'Operator release notes' };
function fixture(options = {}) {
  const calls = [];
  const outputs = {};
  const logs = [];
  let lists = 0;
  let refs = 0;
  const github = {
    rest: {
      git: {
        async getRef(args) {
          calls.push(['getRef', args]);
          if (options.refError) throw options.refError;
          const object = options.refs?.[refs++] || options.ref || { type: 'commit', sha };
          return { data: { object } };
        },
        async getTag(args) {
          calls.push(['getTag', args]);
          return { data: { object: options.tags[args.tag_sha] } };
        },
      },
      repos: {
        listReleases() {},
        async getRelease(args) {
          calls.push(['getRelease', args]);
          if (options.releaseError) throw options.releaseError;
          return { data: options.release || draft };
        },
        async createRelease(args) {
          calls.push(['createRelease', args]);
          if (options.createError) throw options.createError;
          return { data: options.created || draft };
        },
      },
    },
    async paginate(method, args) {
      assert.equal(method, github.rest.repos.listReleases);
      calls.push(['listReleases', args]);
      const result = options.lists?.[lists++] ?? [];
      if (result instanceof Error) throw result;
      return result;
    },
  };
  const input = {
    github,
    releaseId: options.releaseId,
    context: { eventName: 'push', ref: `refs/tags/${tag}`, sha, repo: { owner: 'HifiMule', repo: 'HifiMule' }, ...options.context },
    core: { setOutput: (key, value) => { outputs[key] = value; }, info: (value) => logs.push(value), warning: (value) => logs.push(value) },
  };
  return { run: () => prepareDraftRelease(input), calls, outputs, logs };
}

test('platform reruns revalidate the saved draft ID without listing or creating releases', async () => {
  const f = fixture({ releaseId: '42' });
  await f.run();
  assert.equal(f.outputs['release-id'], '42');
  assert.deepEqual(f.calls.filter(([name]) => !['getRef', 'getTag'].includes(name)), [
    ['getRelease', { owner: 'HifiMule', repo: 'HifiMule', release_id: 42 }],
  ]);
});

for (const [name, options, expected] of [
  ['published draft', { release: { ...draft, draft: false } }, /already published/],
  ['moved tag', { ref: { type: 'commit', sha: 'moved' } }, /Tag moved/],
  ['retagged release', { release: { ...draft, tag_name: 'v0.14.0' } }, /valid matching draft/],
  ['deleted release', { releaseError: Object.assign(new Error('deleted'), { status: 404 }) }, /validate prepared.*HTTP 404/],
  ['missing ID', { releaseId: '' }, /ID is missing or invalid/],
  ['unsafe ID', { releaseId: '9007199254740992' }, /ID is missing or invalid/],
]) {
  test(`platform rerun rejects ${name} without creating a replacement`, async () => {
    const f = fixture({ releaseId: '42', ...options });
    await assert.rejects(f.run(), expected);
    assert.ok(f.calls.every(([name]) => !['listReleases', 'createRelease'].includes(name)));
    assert.deepEqual(f.outputs, {});
  });
}

test('creates one draft for an existing tag and outputs its ID', async () => {
  const f = fixture();
  assert.equal(await f.run(), 42);
  assert.deepEqual(f.outputs, { 'release-id': '42' });
  const creates = f.calls.filter(([name]) => name === 'createRelease');
  assert.equal(creates.length, 1);
  assert.deepEqual(creates[0][1], {
    owner: 'HifiMule', repo: 'HifiMule', tag_name: tag, target_commitish: sha,
    name: `HifiMule ${tag}`, body: 'See the release notes for details.', draft: true, prerelease: false,
  });
});

test('reuses matching draft without replacing notes or writing a release', async () => {
  const f = fixture({ lists: [[{ ...draft, id: 99, tag_name: 'v0.14.0' }, draft]] });
  await f.run();
  assert.equal(f.outputs['release-id'], '42');
  assert.equal(draft.body, 'Operator release notes');
  assert.ok(f.calls.every(([name]) => name !== 'createRelease'));
});

test('dereferences nested annotated tags', async () => {
  const f = fixture({ ref: { type: 'tag', sha: 'tag1' }, tags: {
    tag1: { type: 'tag', sha: 'tag2' }, tag2: { type: 'commit', sha },
  }, lists: [[draft]] });
  await f.run();
  assert.equal(f.calls.filter(([name]) => name === 'getTag').length, 4);
});

for (const [name, options, expected] of [
  ['moved tag', { ref: { type: 'commit', sha: 'b'.repeat(40) } }, /Tag moved: expected .*observed b+/],
  ['noncommit tag', { ref: { type: 'tree', sha } }, /must be a commit/],
  ['cyclic annotation', { ref: { type: 'tag', sha: 'cycle' }, tags: { cycle: { type: 'tag', sha: 'cycle' } } }, /cyclic or exceeds/],
  ['missing tag', { refError: Object.assign(new Error('secret token'), { status: 404 }) }, /resolve tag.*HTTP 404/],
  ['published release', { lists: [[{ ...draft, draft: false }]] }, /already published/],
  ['ambiguous drafts', { lists: [[draft, { ...draft, id: 43 }]] }, /Multiple matching/],
  ['tag moved during lookup', { refs: [{ type: 'commit', sha }, { type: 'commit', sha: 'b'.repeat(40) }] }, /Tag moved/],
]) {
  test(`rejects ${name} before any write`, async () => {
    const f = fixture(options);
    await assert.rejects(f.run(), expected);
    assert.ok(f.calls.every(([operation]) => operation !== 'createRelease'));
    assert.deepEqual(f.outputs, {});
  });
}

test('recovers an ambiguously created draft with one re-list and no write retry', async () => {
  const f = fixture({ lists: [[], [draft]], createError: Object.assign(new Error('request failed'), { status: 502 }) });
  await f.run();
  assert.equal(f.outputs['release-id'], '42');
  assert.equal(f.calls.filter(([name]) => name === 'createRelease').length, 1);
  assert.equal(f.calls.filter(([name]) => name === 'listReleases').length, 2);
});

test('creation denial retains safe status and request ID even if reconciliation fails', async () => {
  const createError = Object.assign(new Error('token=SUPERSECRET'), {
    status: 403, response: { headers: { 'x-github-request-id': 'ABCD:1234', authorization: 'SUPERSECRET' } },
  });
  const f = fixture({ lists: [[], new Error('SUPERSECRET')], createError });
  await assert.rejects(f.run(), (error) => {
    assert.match(error.message, /create draft release.*HTTP 403; request ID ABCD:1234/);
    assert.ok(!error.message.includes('SUPERSECRET'));
    return true;
  });
  assert.ok(f.logs.every((line) => !line.includes('SUPERSECRET')));
  assert.equal(f.calls.filter(([name]) => name === 'createRelease').length, 1);
  assert.deepEqual(f.outputs, {});
});

test('candidate dispatch does not call GitHub or emit a release ID', async () => {
  const f = fixture({ context: { eventName: 'workflow_dispatch', ref: 'refs/heads/main' } });
  await f.run();
  assert.deepEqual(f.calls, []);
  assert.deepEqual(f.outputs, {});
});

test('rejects invalid creation response without exposing an ID', async () => {
  const f = fixture({ created: { ...draft, id: null } });
  await assert.rejects(f.run(), /valid matching draft release ID/);
  assert.deepEqual(f.outputs, {});
});

for (const [name, options, writes] of [
  ['existing draft lookup', { lists: [[draft]], refs: [{ type: 'commit', sha }, { type: 'commit', sha: 'moved' }] }, 0],
  ['successful creation', { refs: [{ type: 'commit', sha }, { type: 'commit', sha }, { type: 'commit', sha: 'moved' }] }, 1],
]) {
  test(`withholds release ID if tag moves during ${name}`, async () => {
    const f = fixture(options);
    await assert.rejects(f.run(), /Tag moved/);
    assert.deepEqual(f.outputs, {});
    assert.equal(f.calls.filter(([operation]) => operation === 'createRelease').length, writes);
  });
}
