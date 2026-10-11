// build-extensions-cache.mjs のテスト（node --test。`pnpm test` から実行する）
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, readFile, rm, stat, utimes, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { after, before, describe, it } from 'node:test';
import {
	CACHE_VERSION,
	hashInputs,
	readCache,
	shouldSkip,
	syncFile,
	syncTree,
	writeCache
} from './build-extensions-cache.mjs';

let dir;

before(async () => {
	dir = await mkdtemp(join(tmpdir(), 'hikae-build-ext-test-'));
});

after(async () => {
	await rm(dir, { recursive: true, force: true });
});

describe('shouldSkip', () => {
	const cache = { version: CACHE_VERSION, inputHash: 'abc' };

	it('入力が同じで出力が揃っていれば飛ばす', () => {
		assert.equal(shouldSkip({ force: false, cache, inputHash: 'abc', outputsPresent: true }), true);
	});

	it('--force のときは必ずビルドする', () => {
		assert.equal(shouldSkip({ force: true, cache, inputHash: 'abc', outputsPresent: true }), false);
	});

	it('入力が変わったらビルドする', () => {
		assert.equal(
			shouldSkip({ force: false, cache, inputHash: 'xyz', outputsPresent: true }),
			false
		);
	});

	it('出力が欠けていればビルドする', () => {
		assert.equal(
			shouldSkip({ force: false, cache, inputHash: 'abc', outputsPresent: false }),
			false
		);
	});

	it('キャッシュが無い・形式が古い・版が違うならビルドする', () => {
		assert.equal(
			shouldSkip({ force: false, cache: null, inputHash: 'abc', outputsPresent: true }),
			false
		);
		assert.equal(
			shouldSkip({
				force: false,
				cache: { inputHash: 'abc' },
				inputHash: 'abc',
				outputsPresent: true
			}),
			false
		);
		assert.equal(
			shouldSkip({
				force: false,
				cache: { version: CACHE_VERSION + 1, inputHash: 'abc' },
				inputHash: 'abc',
				outputsPresent: true
			}),
			false
		);
	});
});

describe('hashInputs', () => {
	it('内容が同じなら同じハッシュ、更新時刻だけ変えても変わらない', async () => {
		const root = join(dir, 'hash-same');
		await mkdir(join(root, 'src'), { recursive: true });
		await writeFile(join(root, 'src', 'a.ts'), 'export const a = 1;\n');
		const first = await hashInputs(root, ['src']);
		await utimes(join(root, 'src', 'a.ts'), new Date(0), new Date(0));
		assert.equal(await hashInputs(root, ['src']), first);
	});

	it('内容が変わると別のハッシュになる', async () => {
		const root = join(dir, 'hash-changed');
		await mkdir(join(root, 'src'), { recursive: true });
		await writeFile(join(root, 'src', 'a.ts'), 'export const a = 1;\n');
		const first = await hashInputs(root, ['src']);
		await writeFile(join(root, 'src', 'a.ts'), 'export const a = 2;\n');
		assert.notEqual(await hashInputs(root, ['src']), first);
	});

	it('ファイルの追加・存在しない入力も変化として扱う', async () => {
		const root = join(dir, 'hash-missing');
		await mkdir(join(root, 'src'), { recursive: true });
		await writeFile(join(root, 'src', 'a.ts'), 'x\n');
		const before = await hashInputs(root, ['src', 'lock.json']);
		await writeFile(join(root, 'lock.json'), '{}\n');
		assert.notEqual(await hashInputs(root, ['src', 'lock.json']), before);
		await writeFile(join(root, 'src', 'b.ts'), 'y\n');
		const withB = await hashInputs(root, ['src', 'lock.json']);
		assert.notEqual(withB, before);
	});

	it('入力の順序に依存しない', async () => {
		const root = join(dir, 'hash-order');
		await mkdir(join(root, 'a'), { recursive: true });
		await writeFile(join(root, 'a', 'x.txt'), 'x\n');
		await writeFile(join(root, 'y.txt'), 'y\n');
		assert.equal(await hashInputs(root, ['a', 'y.txt']), await hashInputs(root, ['y.txt', 'a']));
	});
});

describe('syncFile', () => {
	it('内容が同じなら書き換えず、更新時刻も変えない', async () => {
		const root = join(dir, 'sync-file');
		await mkdir(root, { recursive: true });
		const src = join(root, 'src.bin');
		const dest = join(root, 'dest.bin');
		await writeFile(src, Buffer.from([1, 2, 3]));
		assert.equal(await syncFile(src, dest), true);
		await utimes(dest, new Date(0), new Date(0));
		assert.equal(await syncFile(src, dest), false);
		assert.equal((await stat(dest)).mtimeMs, 0);
		await writeFile(src, Buffer.from([1, 2, 4]));
		assert.equal(await syncFile(src, dest), true);
		assert.deepEqual(await readFile(dest), Buffer.from([1, 2, 4]));
	});
});

describe('syncTree', () => {
	it('違うファイルだけ書き、無くなったファイルを消す', async () => {
		const src = join(dir, 'tree-src');
		const dest = join(dir, 'tree-dest');
		await mkdir(join(src, 'assets'), { recursive: true });
		await mkdir(join(dest, 'assets'), { recursive: true });
		await writeFile(join(src, 'index.html'), 'same\n');
		await writeFile(join(dest, 'index.html'), 'same\n');
		await utimes(join(dest, 'index.html'), new Date(0), new Date(0));
		await writeFile(join(src, 'assets', 'new.js'), 'new\n');
		await writeFile(join(src, 'assets', 'app.css'), 'v2\n');
		await writeFile(join(dest, 'assets', 'app.css'), 'v1\n');
		await writeFile(join(dest, 'assets', 'old.js'), 'old\n');

		const result = await syncTree(src, dest);
		assert.deepEqual(result.written.sort(), ['assets/app.css', 'assets/new.js']);
		assert.deepEqual(result.removed, ['assets/old.js']);
		assert.equal(result.unchanged, 1);
		assert.equal((await stat(join(dest, 'index.html'))).mtimeMs, 0);
		assert.equal(await readFile(join(dest, 'assets', 'app.css'), 'utf8'), 'v2\n');
	});
});

describe('readCache / writeCache', () => {
	it('書いた値を読み戻す。無い・壊れている場合は null', async () => {
		const path = join(dir, 'cache', 'state.json');
		assert.equal(await readCache(path), null);
		await writeCache(path, 'hash-1');
		assert.deepEqual(await readCache(path), { version: CACHE_VERSION, inputHash: 'hash-1' });
		await writeFile(path, '{ broken');
		assert.equal(await readCache(path), null);
	});
});
