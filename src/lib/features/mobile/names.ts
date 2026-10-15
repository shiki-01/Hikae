// 既定のファイル名（メモ・クリップボード）と、重複しない名前の作り方。

const pad = (value: number): string => String(value).padStart(2, '0');

/** `YYYY-MM-DD_HHmm`（端末の時刻）。既定名の日時の部分 */
export function timestampStamp(now: Date): string {
	const date = `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
	return `${date}_${pad(now.getHours())}${pad(now.getMinutes())}`;
}

/** 名前を、拡張子の前とその後ろに分ける（拡張子が無ければ空）。`a.tar.gz` は `a.tar` と `gz` */
export function splitExtension(name: string): { stem: string; ext: string } {
	const dot = name.lastIndexOf('.');
	if (dot <= 0) return { stem: name, ext: '' };
	return { stem: name.slice(0, dot), ext: name.slice(dot + 1) };
}

/**
 * 既存の名前と重ならない名前を返す。重なるときは `名前 (2).拡張子` の形（リポジトリ側の「両方残す」と同じ）。
 * 比較は大文字と小文字を区別しない（GitHub 上で別ファイルとして扱えない名前を避ける）
 */
export function uniqueName(name: string, taken: ReadonlySet<string>): string {
	const lowered = new Set([...taken].map((value) => value.toLowerCase()));
	if (!lowered.has(name.toLowerCase())) return name;
	const { stem, ext } = splitExtension(name);
	const suffix = (n: number) => (ext === '' ? `${stem} (${n})` : `${stem} (${n}).${ext}`);
	for (let n = 2; ; n += 1) {
		const candidate = suffix(n);
		if (!lowered.has(candidate.toLowerCase())) return candidate;
	}
}

/** 既定のメモ名（`メモ_YYYY-MM-DD_HHmm.txt`） */
export function memoFileName(now: Date, taken: ReadonlySet<string>): string {
	return uniqueName(`メモ_${timestampStamp(now)}.txt`, taken);
}

/** 既定のクリップボードの名前（`クリップボード_YYYY-MM-DD_HHmm.拡張子`） */
export function clipboardFileName(
	now: Date,
	extension: string,
	taken: ReadonlySet<string>
): string {
	return uniqueName(`クリップボード_${timestampStamp(now)}.${extension}`, taken);
}
