// 拡張機能の一覧の表示に使う、純粋な関数（設計書 11.9、S8）。

/**
 * 対応する形式の表示（例: `.docx、.xlsx、そのほかのすべてのファイル`）。
 * 拡張子は `.` を付けて重複を除き、全部の拡張子を表す `*` は `anyLabel` に置き換えて末尾に付ける。
 * 形式が 1 つも無いときは空文字を返す（呼び出し側が「対応する形式はありません」を出す）
 */
export function formatFileTypes(fileTypes: string[], anyLabel: string): string {
	const labels: string[] = [];
	for (const type of fileTypes) {
		if (type === '*') continue;
		const label = `.${type}`;
		if (!labels.includes(label)) labels.push(label);
	}
	if (fileTypes.includes('*')) labels.push(anyLabel);
	return labels.join('、');
}
