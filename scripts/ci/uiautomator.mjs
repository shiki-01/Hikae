// uiautomator dump の XML を調べる補助（Android の画面確認用。docs/notes/mobile-testflight.md）。
// 使い方:
//   node scripts/ci/uiautomator.mjs center <xml> <文字列>   … その文字列を含む要素の中心座標 "x y" を出す（無ければ終了コード 1）
//   node scripts/ci/uiautomator.mjs match <xml> <正規表現>  … 画面の文字に一致するものがあれば、その文字列を出す（無ければ終了コード 1）
import { readFileSync } from 'node:fs';

const [, , command, file, pattern] = process.argv;
const xml = readFileSync(file, 'utf8');

/** text / content-desc / bounds を持つ node を取り出す */
function nodes() {
	const result = [];
	for (const m of xml.matchAll(/<node\b[^>]*>/g)) {
		const tag = m[0];
		const attr = (name) => {
			const hit = tag.match(new RegExp(`\\b${name}="([^"]*)"`));
			return hit ? hit[1].replaceAll('&amp;', '&').replaceAll('&quot;', '"') : '';
		};
		result.push({
			text: attr('text'),
			desc: attr('content-desc'),
			bounds: attr('bounds')
		});
	}
	return result;
}

if (command === 'center') {
	// 文字が完全に一致する要素を優先する（説明文の中に同じ文字列が含まれることがあるため）。無ければ部分一致
	const all = nodes();
	const exact = all.filter((n) => n.text === pattern || n.desc === pattern);
	const partial = all.filter((n) => n.text.includes(pattern) || n.desc.includes(pattern));
	for (const node of [...exact, ...partial]) {
		const b = node.bounds.match(/\[(\d+),(\d+)\]\[(\d+),(\d+)\]/);
		if (!b) continue;
		const [x1, y1, x2, y2] = b.slice(1).map(Number);
		console.log(`${Math.round((x1 + x2) / 2)} ${Math.round((y1 + y2) / 2)}`);
		process.exit(0);
	}
	process.exit(1);
} else if (command === 'match') {
	const re = new RegExp(pattern);
	for (const node of nodes()) {
		const hit = (node.text + '\n' + node.desc).match(re);
		if (hit) {
			console.log(hit[0]);
			process.exit(0);
		}
	}
	process.exit(1);
} else {
	console.error('unknown command');
	process.exit(2);
}
