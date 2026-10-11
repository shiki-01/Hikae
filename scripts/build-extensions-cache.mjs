// build-extensions.mjs が使う、入力の内容ハッシュ・キャッシュの判定・出力の差分配置。
//
// - 入力のハッシュ: 組み込みの拡張（wasm と表示拡張の UI）のビルドに効くファイルの内容から作る。
//   更新時刻は使わない（内容が同じなら同じハッシュになる）
// - 差分配置: 既存の出力と内容が違うファイルだけを書き換え、不要になったファイルを消す。
//   内容が同じファイルは更新時刻を変えない（tauri-build の rerun-if-changed を不要に起こさないため）

import { createHash } from 'node:crypto';
import { mkdir, readFile, readdir, stat, unlink, writeFile } from 'node:fs/promises';
import { dirname, join, relative, sep } from 'node:path';

// キャッシュの形式を変えたら上げる（古いキャッシュを無効にする）
export const CACHE_VERSION = 1;

/** ディレクトリ以下の通常ファイルの絶対パスを返す。存在しなければ空配列 */
export async function listFiles(dir) {
	let entries;
	try {
		entries = await readdir(dir, { withFileTypes: true });
	} catch (err) {
		if (err?.code === 'ENOENT') return [];
		throw err;
	}
	const files = [];
	for (const entry of entries) {
		const path = join(dir, entry.name);
		if (entry.isDirectory()) {
			files.push(...(await listFiles(path)));
		} else if (entry.isFile()) {
			files.push(path);
		}
	}
	return files;
}

function toPosix(path) {
	return path.split(sep).join('/');
}

/**
 * 入力（root からの相対パスのファイルまたはディレクトリ）の内容ハッシュを返す。
 * 存在しない入力は「無い」という記録として含めるため、ファイルの追加・削除も変化になる。
 */
export async function hashInputs(root, inputs) {
	const records = [];
	for (const input of inputs) {
		const abs = join(root, input);
		const info = await stat(abs).catch(() => null);
		if (info?.isDirectory()) {
			for (const file of await listFiles(abs)) {
				records.push({ rel: toPosix(relative(root, file)), file });
			}
		} else if (info?.isFile()) {
			records.push({ rel: toPosix(input), file: abs });
		} else {
			// 存在しない入力は、無いことを記録する（追加されたときにハッシュが変わるように）
			records.push({ rel: toPosix(input), file: null });
		}
	}
	records.sort((a, b) => (a.rel < b.rel ? -1 : a.rel > b.rel ? 1 : 0));

	const hash = createHash('sha256');
	for (const { rel, file } of records) {
		hash.update(rel);
		hash.update('\0');
		if (file === null) {
			hash.update('missing\n');
			continue;
		}
		const content = await readFile(file);
		hash.update(createHash('sha256').update(content).digest('hex'));
		hash.update('\n');
	}
	return hash.digest('hex');
}

/**
 * ビルドを飛ばしてよいかを判定する（純関数）。
 * 前回と入力が同じで、出力がすべて揃っていて、強制でないときだけ飛ばす。
 */
export function shouldSkip({ force, cache, inputHash, outputsPresent }) {
	if (force) return false;
	if (!outputsPresent) return false;
	if (!cache || typeof cache !== 'object') return false;
	return cache.version === CACHE_VERSION && cache.inputHash === inputHash;
}

/** キャッシュを読む。無い・壊れている場合は null（ビルドする側に倒す） */
export async function readCache(path) {
	try {
		return JSON.parse(await readFile(path, 'utf8'));
	} catch {
		return null;
	}
}

/** キャッシュを書く（ビルドが全部成功したあとに限る） */
export async function writeCache(path, inputHash) {
	await mkdir(dirname(path), { recursive: true });
	await writeFile(path, JSON.stringify({ version: CACHE_VERSION, inputHash }, null, 2) + '\n');
}

/**
 * src のファイルを dest に置く。内容が同じなら何もしない（更新時刻を変えない）。
 * 戻り値: 書き換えたら true
 */
export async function syncFile(src, dest) {
	const content = await readFile(src);
	try {
		const current = await readFile(dest);
		if (current.equals(content)) return false;
	} catch (err) {
		if (err?.code !== 'ENOENT') throw err;
	}
	await mkdir(dirname(dest), { recursive: true });
	await writeFile(dest, content);
	return true;
}

/**
 * src ディレクトリの内容を dest ディレクトリに反映する。
 * 内容が違うファイルだけ書き換え、src に無いファイルを dest から消す。
 * 戻り値: { written, removed, unchanged }（相対パスの配列と件数）
 */
export async function syncTree(srcDir, destDir) {
	const srcFiles = await listFiles(srcDir);
	const srcRels = new Set();
	const written = [];
	let unchanged = 0;
	for (const src of srcFiles) {
		const rel = toPosix(relative(srcDir, src));
		srcRels.add(rel);
		if (await syncFile(src, join(destDir, rel))) {
			written.push(rel);
		} else {
			unchanged += 1;
		}
	}
	const removed = [];
	for (const dest of await listFiles(destDir)) {
		const rel = toPosix(relative(destDir, dest));
		if (!srcRels.has(rel)) {
			await unlink(dest);
			removed.push(rel);
		}
	}
	return { written, removed, unchanged };
}
