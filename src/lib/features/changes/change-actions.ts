import type { Change } from '#lib/api/types.js';

export interface ChangeActions {
	/**
	 * 「元に戻す」が、保存済みの版へ戻す操作ではなく、新規ファイルを「元に戻す（作成しない）」になる。
	 * 一度も保存されておらず、登録もされていない新規ファイル（未追跡）だけが対象
	 */
	discard: boolean;
}

/** 変更の行・詳細に出す操作を決める */
export function actionsFor(change: Change): ChangeActions {
	return { discard: change.type === 'added' && change.untracked === true && !change.isConflict };
}
