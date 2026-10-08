import type { LargeFile, SizeCheck, SizeChoice } from '#lib/api/types.js';

export interface SizeCheckRow {
	/** バックエンドへ選択として返すときのパス（変換しない） */
	path: string;
	/** 画面に出すパス（区切りを / にそろえる） */
	label: string;
	sizeBytes: number | null;
	/** 100MB を超え、保存できない */
	blocked: boolean;
}

export interface SizeCheckView {
	/** 保存できないファイルが先、続いて警告のファイル */
	rows: SizeCheckRow[];
	hasBlocked: boolean;
	/** 「そのまま保存する」を出せるか。保存できないファイルがあれば出せない */
	canAccept: boolean;
}

function toRow(file: LargeFile, blocked: boolean): SizeCheckRow {
	return {
		path: file.path,
		label: file.path.replaceAll('\\', '/'),
		sizeBytes: file.sizeBytes,
		blocked
	};
}

export function viewSizeCheck(check: SizeCheck): SizeCheckView {
	const hasBlocked = check.blocked.length > 0;
	return {
		rows: [
			...check.blocked.map((f) => toRow(f, true)),
			...check.warned.map((f) => toRow(f, false))
		],
		hasBlocked,
		canAccept: !hasBlocked
	};
}

export type SizeAction = 'accept' | 'exclude';

/**
 * 選択肢から、保存をやり直すための選択を作る。
 * 「そのまま保存」は警告だけのときに限り、保存できないファイルがある場合は null（出さない）。
 * 「外す」は検査で見つかったファイルをすべて保存対象から外す。
 */
export function choiceFor(check: SizeCheck, action: SizeAction): SizeChoice | null {
	if (action === 'accept') {
		if (check.blocked.length > 0) return null;
		return { acceptWarned: true, exclude: [] };
	}
	return {
		acceptWarned: false,
		exclude: [...check.blocked, ...check.warned].map((f) => f.path)
	};
}
