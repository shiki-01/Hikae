// アップロード（追加して保存）の下書き。タブを移っても残るよう、画面の外（モジュール）に持つ。
// 項目の変更は upload-items.ts の純関数で行い、実行の状態は nextRunState で遷移させる。

import type { UploadPlan, UploadResult, OnExists } from '#lib/api/mobile-types.js';
import {
	applyPlan,
	conflictItems,
	createItem,
	nextRunState,
	removeItem,
	renameItem,
	setOnExists,
	type NewUploadItem,
	type RunEvent,
	type RunState,
	type UploadItem
} from '#lib/features/mobile/upload-items.js';

interface Draft {
	/** 追加先のフォルダ（ルートは空文字） */
	folder: string;
	items: UploadItem[];
	run: RunState;
	/** 保存メッセージの編集値。null のときは既定の文言を使う */
	messageDraft: string | null;
	result: UploadResult | null;
}

export const draft: Draft = $state({
	folder: '',
	items: [],
	run: 'editing',
	messageDraft: null,
	result: null
});

function emit(event: RunEvent): void {
	draft.run = nextRunState(draft.run, event);
}

/** 下書きを空にする（追加先は指定したフォルダ） */
export function resetDraft(folder: string): void {
	draft.folder = folder;
	draft.items = [];
	draft.run = 'editing';
	draft.messageDraft = null;
	draft.result = null;
}

/** 項目が無いときだけ、追加先を開いているフォルダに合わせる */
export function syncUploadFolder(folder: string): void {
	if (draft.items.length === 0 && draft.run === 'editing') draft.folder = folder;
}

export function setDraftFolder(folder: string): void {
	if (draft.run === 'uploading' || draft.folder === folder) return;
	draft.folder = folder;
	draft.items = draft.items.map((item) => ({ ...item, exists: false }));
	emit('edit');
}

export function addDraftItems(items: NewUploadItem[]): void {
	if (draft.run === 'uploading' || items.length === 0) return;
	draft.items = [...draft.items, ...items.map(createItem)];
	emit('edit');
}

export function renameDraftItem(id: string, name: string): void {
	if (draft.run === 'uploading') return;
	draft.items = renameItem(draft.items, id, name);
	emit('edit');
}

export function removeDraftItem(id: string): void {
	if (draft.run === 'uploading') return;
	draft.items = removeItem(draft.items, id);
	emit('edit');
}

/** 同名のときの扱いを選ぶ。確認の結果は変わらないので、実行の状態も変えない */
export function setDraftOnExists(id: string, onExists: OnExists): void {
	if (draft.run === 'uploading') return;
	draft.items = setOnExists(draft.items, id, onExists);
}

export function setDraftMessage(value: string | null): void {
	draft.messageDraft = value;
}

/** 確認（plan）の結果を入れる。同名があれば「確認済み」（もう一度押すと保存）、無ければそのまま保存へ進む */
export function applyDraftPlan(plan: UploadPlan): boolean {
	draft.items = applyPlan(draft.items, plan);
	const hasConflicts = conflictItems(draft.items).length > 0;
	emit(hasConflicts ? 'planned_conflict' : 'planned_clear');
	return hasConflicts;
}

/** 「確認済み」から保存を始める */
export function startDraftUpload(): void {
	emit('start');
}

export function finishDraftUpload(result: UploadResult): void {
	draft.result = result;
	emit('succeeded');
}

/** 保存に失敗した。項目は残っているので、編集中へ戻す（データは無事） */
export function failDraftUpload(): void {
	emit('failed');
}
