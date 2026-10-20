#!/usr/bin/env node
// 拡張機能の雛形を作る CLI（設計書 11.2・11.3・11.7）。
//
//   node scripts/create-extension.mjs <id> --name "表示名" --out <出力先フォルダ>
//
// 出力は extension.json（apiVersion "1"、permissions 空、renderers に 1 件の例）、ui/index.html・ui/main.js
// （プロトコル v1 の最小の受け取り例。権限は使わない）、README.md（開発者モードでの追加の手順）。
// 依存は Node の標準モジュールのみ。出力先に既にファイルがあれば何も書かずに失敗する（削除はしない）。
//
// 検証の規則（id の形式）は Rust の `valid_id`（core-preview の extension/id.rs）と同じにする。

import { mkdirSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

/** 雛形の拡張子（11.2 の fileTypes）。実在のファイルに効かないよう、架空の拡張子にする */
const SAMPLE_FILE_TYPE = '.sample';

/** `id` の形式（設計書 11.2）: `^[a-z0-9]+([.-][a-z0-9]+)*$`（Rust の `valid_id` と同じ） */
const ID_PATTERN = /^[a-z0-9]+(?:[.-][a-z0-9]+)*$/;

/** 雛形の作成の失敗。メッセージは利用者に見せてよい文言（パス以外の内容を含めない） */
export class CreateExtensionError extends Error {
	constructor(message) {
		super(message);
		this.name = 'CreateExtensionError';
	}
}

/**
 * `id` が拡張の id の形式に合うか（11.2）。
 * @param {unknown} id
 * @returns {boolean}
 */
export function isValidExtensionId(id) {
	return typeof id === 'string' && ID_PATTERN.test(id);
}

/** HTML の本文・属性に入れる文字列を退避する */
function escapeHtml(text) {
	return text
		.replaceAll('&', '&amp;')
		.replaceAll('<', '&lt;')
		.replaceAll('>', '&gt;')
		.replaceAll('"', '&quot;')
		.replaceAll("'", '&#39;');
}

/**
 * 雛形のファイルの内容を作る（ファイルには書かない）。
 * @param {{ id: string, name: string }} options
 * @returns {Array<{ path: string, content: string }>} `/` 区切りの相対パスと内容
 */
export function buildExtensionFiles({ id, name }) {
	const manifest = {
		id,
		name,
		version: '0.1.0',
		apiVersion: '1',
		contributes: {
			renderers: [{ fileTypes: [SAMPLE_FILE_TYPE], modes: ['view'], entry: 'ui/index.html' }],
			extractors: []
		},
		permissions: []
	};
	const title = escapeHtml(name);

	const indexHtml = `<!doctype html>
<html lang="ja">
<head>
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width, initial-scale=1" />
<title>${title}</title>
<style>
html, body { margin: 0; height: 100%; font-family: system-ui, sans-serif; }
body { padding: 16px; box-sizing: border-box; }
pre { white-space: pre-wrap; word-break: break-word; margin: 0; }
p.notice { color: #555; }
</style>
</head>
<body>
<p class="notice" id="notice">読み込み中です。</p>
<pre id="content" hidden></pre>
<script src="main.js"></script>
</body>
</html>
`;

	// 受け取りは v1 のプロトコル（設計書 11.7）。ホストからの render だけを描き、内容は textContent で入れる
	const mainJs = `// 表示拡張の受け取り例（設計書 11.7・プロトコル v1）。
// ホストから render（mode "view"）を受け取り、テキストだけを描く。それ以外は unsupported として返す。
// 権限は使わない（通信・ファイル・Tauri の呼び出しはできない）。

const PROTOCOL_VERSION = 1;
const notice = document.getElementById('notice');
const content = document.getElementById('content');

/** ホストへ送る。origin は opaque のため "*" を使う */
function send(message) {
  window.parent.postMessage({ v: PROTOCOL_VERSION, ...message }, '*');
}

function showNotice(text) {
  notice.textContent = text;
  notice.hidden = false;
  content.hidden = true;
}

window.addEventListener('message', (event) => {
  // 親（ホスト）以外からのメッセージは受け付けない
  if (event.source !== window.parent) return;
  const data = event.data;
  if (!data || data.v !== PROTOCOL_VERSION || data.type !== 'render' || data.mode !== 'view') return;

  const id = data.id;
  const file = data.file || {};
  if (file.state !== 'ok') {
    showNotice('このファイルは表示できません。');
    return;
  }
  if (data.content && data.content.kind === 'text') {
    notice.hidden = true;
    content.textContent = data.content.text;
    content.hidden = false;
    return;
  }
  // このサンプルが扱わない内容の種類は、対応しないと返す
  showNotice('この形式は、この表示拡張では表示できません。');
  send({ type: 'error', id, code: 'unsupported' });
});

// 読み込みの完了は 1 回だけ知らせる。ホストはこれを待ってから render を送る
send({ type: 'ready' });
`;

	const readme = `# ${name}（${id}）

Hikae の表示拡張の雛形です。ファイル（${SAMPLE_FILE_TYPE}）の中身を、ui/ の画面で表示します。

## 開発者モードで追加する

1. Hikae の設定を開き、「拡張機能」を選びます。
2. 「開発者モード」をオンにします。確認が出たら、作者を信頼できる拡張だけを追加すると確認してオンにします。
3. 「フォルダから追加」を選び、このフォルダ（extension.json がある場所）を選びます。
4. Hikae を一度閉じて、もう一度開きます（追加した拡張は次回の起動から有効になります）。

## 試す

${SAMPLE_FILE_TYPE} の拡張子のファイルを表示すると、この拡張の画面が出ます。

## 作り直す

- \`extension.json\` の \`fileTypes\` を、実際に扱いたい拡張子に変えます。標準の表示が個別に扱う拡張子（例: \`.pdf\`、\`.png\`、\`.docx\`）を書くと、同じ拡張子の宣言が重なって登録されません。
- \`ui/main.js\` で、表示の内容を変えます。受け取りは「ready を 1 回送る → render を受け取る → error で失敗を返す」の順です（設計書 11.7）。
- 権限（\`permissions\`）は空のままにします。空でない拡張は受け付けません。
- 通信（\`fetch\` など）・ファイル・Tauri の呼び出しは、この拡張では使えません（iframe の CSP で止められます）。

## 注意

- 署名のない拡張は、開いたファイルの内容を読めます。信頼できる作者の拡張だけを追加してください。
`;

	return [
		{ path: 'extension.json', content: `${JSON.stringify(manifest, null, 2)}\n` },
		{ path: 'ui/index.html', content: indexHtml },
		{ path: 'ui/main.js', content: mainJs },
		{ path: 'README.md', content: readme }
	];
}

/**
 * 雛形を出力先に作る。出力先に既に何かがあれば、何も書かずに失敗する。
 * @param {{ id: string, name: string, outDir: string }} options
 * @returns {string[]} 書いたファイルの絶対パス
 */
export function createExtension({ id, name, outDir }) {
	if (!isValidExtensionId(id)) {
		throw new CreateExtensionError(
			'id の形式が正しくありません（小文字・数字と、区切りの . または - だけ。例: example.my-preview）'
		);
	}
	if (typeof name !== 'string' || name.trim() === '') {
		throw new CreateExtensionError('--name に表示名を指定してください');
	}
	if (typeof outDir !== 'string' || outDir === '') {
		throw new CreateExtensionError('--out に出力先のフォルダを指定してください');
	}

	const files = buildExtensionFiles({ id, name: name.trim() });

	// 既存を確かめてから書く（途中で失敗した出力を残さないため、先にすべて検査する）
	let existing = false;
	try {
		const stat = statSync(outDir);
		if (!stat.isDirectory()) {
			throw new CreateExtensionError('出力先が、フォルダではなくファイルです');
		}
		existing = readdirSync(outDir).length > 0;
	} catch (error) {
		if (error instanceof CreateExtensionError) throw error;
		// 無ければ作る（存在しない場合のエラーは正常）
		if (error?.code !== 'ENOENT') throw error;
	}
	if (existing) {
		throw new CreateExtensionError(
			'出力先に既にファイルがあります。空のフォルダを指定してください'
		);
	}

	const written = [];
	for (const file of files) {
		const absolute = join(outDir, ...file.path.split('/'));
		mkdirSync(dirname(absolute), { recursive: true });
		// 'wx' は既存のファイルを上書きしない
		writeFileSync(absolute, file.content, { flag: 'wx' });
		written.push(absolute);
	}
	return written;
}

/** コマンドラインの入口 */
function main(argv) {
	let parsed;
	try {
		parsed = parseArgs({
			args: argv,
			allowPositionals: true,
			options: {
				name: { type: 'string' },
				out: { type: 'string' }
			}
		});
	} catch (error) {
		console.error(`引数が正しくありません: ${error.message}`);
		return 1;
	}
	const [id, extra] = parsed.positionals;
	if (id === undefined || extra !== undefined) {
		console.error(
			'使い方: node scripts/create-extension.mjs <id> --name "表示名" --out <出力先フォルダ>'
		);
		return 1;
	}
	try {
		const written = createExtension({ id, name: parsed.values.name, outDir: parsed.values.out });
		console.log(`雛形を作りました（${written.length} ファイル）: ${parsed.values.out}`);
		return 0;
	} catch (error) {
		if (error instanceof CreateExtensionError) {
			console.error(error.message);
			return 1;
		}
		throw error;
	}
}

// 直接実行されたときだけ入口を動かす（テストから import しても動かさない）
if (process.argv[1] === fileURLToPath(import.meta.url)) {
	process.exitCode = main(process.argv.slice(2));
}
