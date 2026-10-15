// アップロード（追加して保存）の項目と、実行の状態遷移の純関数（docs/design.md 14.5・14.7 M4）。
// 画面は項目を直接書き換えず、ここの関数で新しい配列を作る。

import type {
	OnExists,
	PlanItemInput,
	UploadItemInput,
	UploadPlan
} from '#lib/api/mobile-types.js';

/** 項目の出どころ。ファイル・写真・テキスト・クリップボードのどれか */
export type UploadSource = 'file' | 'photo' | 'text' | 'clipboard';

export interface UploadItem {
	id: string;
	source: UploadSource;
	/** 保存する名前（利用者が編集できる） */
	name: string;
	/** バイト数 */
	size: number;
	contentBase64: string;
	/** 同名があったときの扱い。既定は「両方残す」（既存のファイルを消さない側） */
	onExists: OnExists;
	/** 直近の確認（plan）で、同じパスのファイルが既にあったか */
	exists: boolean;
}

export type NewUploadItem = Omit<UploadItem, 'onExists' | 'exists'>;

export function createItem(input: NewUploadItem): UploadItem {
	return { ...input, onExists: 'keepBoth', exists: false };
}

/** 名前を変える。確認の結果は古くなるので、同名の判定を取り消す */
export function renameItem(items: readonly UploadItem[], id: string, name: string): UploadItem[] {
	return items.map((item) => (item.id === id ? { ...item, name, exists: false } : item));
}

export function removeItem(items: readonly UploadItem[], id: string): UploadItem[] {
	return items.filter((item) => item.id !== id);
}

/** 同名のときの扱いを選ぶ。確認の結果には影響しない */
export function setOnExists(
	items: readonly UploadItem[],
	id: string,
	onExists: OnExists
): UploadItem[] {
	return items.map((item) => (item.id === id ? { ...item, onExists } : item));
}

/** 確認（plan）の結果を項目に反映する。順序は入力と同じ */
export function applyPlan(items: readonly UploadItem[], plan: UploadPlan): UploadItem[] {
	return items.map((item, index) => ({ ...item, exists: plan.items[index]?.exists ?? false }));
}

export function planInputs(items: readonly UploadItem[], targetDir: string): PlanItemInput[] {
	return items.map((item) => ({ targetDir, name: item.name, size: item.size }));
}

export function uploadInputs(items: readonly UploadItem[], targetDir: string): UploadItemInput[] {
	return items.map((item) => ({
		targetDir,
		name: item.name,
		contentBase64: item.contentBase64,
		onExists: item.onExists
	}));
}

/** 同名のファイルがあり、3 択の確認が必要な項目 */
export function conflictItems(items: readonly UploadItem[]): UploadItem[] {
	return items.filter((item) => item.exists);
}

/**
 * 保存する項目が 1 つでもあるか。全部が「追加しない」の同名だけなら、保存を始めない。
 */
export function hasSaveTargets(items: readonly UploadItem[]): boolean {
	return items.some((item) => !item.exists || item.onExists !== 'skip');
}

export type NameProblem = 'empty' | 'invalid' | 'duplicate';

/** 名前の検査。Windows・GitHub で扱えない細かい規則は Rust 側でも検査する（ここは入力中の明らかな誤り） */
export function nameProblem(
	name: string,
	items: readonly UploadItem[],
	selfId: string
): NameProblem | null {
	const trimmed = name.trim();
	if (trimmed === '') return 'empty';
	// 区切り文字・制御文字・「.」「..」は名前として使えない
	// eslint-disable-next-line no-control-regex
	if (/[/\\\u0000-\u001f\u007f]/.test(name) || trimmed === '.' || trimmed === '..') {
		return 'invalid';
	}
	const lowered = trimmed.toLowerCase();
	const duplicated = items.some(
		(item) => item.id !== selfId && item.name.trim().toLowerCase() === lowered
	);
	return duplicated ? 'duplicate' : null;
}

/** 実行の状態。editing（編集中）→ checked（同名の確認済み。もう一度押すと保存）→ uploading → done */
export type RunState = 'editing' | 'checked' | 'uploading' | 'done';

export type RunEvent =
	'edit' | 'planned_conflict' | 'planned_clear' | 'start' | 'succeeded' | 'failed' | 'reset';

/**
 * 実行の状態遷移。想定外の組み合わせ（保存中の編集など）は状態を変えない。
 * 保存に失敗したときは項目が残っているので editing へ戻す（データは無事）。
 */
export function nextRunState(state: RunState, event: RunEvent): RunState {
	switch (state) {
		case 'editing':
			if (event === 'edit' || event === 'reset') return 'editing';
			if (event === 'planned_conflict') return 'checked';
			if (event === 'planned_clear') return 'uploading';
			return state;
		case 'checked':
			if (event === 'edit') return 'editing';
			if (event === 'start') return 'uploading';
			return state;
		case 'uploading':
			if (event === 'succeeded') return 'done';
			if (event === 'failed') return 'editing';
			return state;
		case 'done':
			if (event === 'edit' || event === 'reset') return 'editing';
			return state;
	}
}
