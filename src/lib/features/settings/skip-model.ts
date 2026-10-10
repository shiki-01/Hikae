import type { SkipWorktreeEntry } from '#lib/api/types.js';
import type { MessageKey } from '#lib/i18n/index.js';

/** 通知に載せる件の名前は、先頭の数件だけにする（残りは件数で示す） */
export const NOTICE_NAME_LIMIT = 3;

/** 「今後の変更だけ無視する」の再適用の結果（取り込み・解消・中止・復旧の後に届く） */
export interface SkipNoticeInput {
	/** 手元の内容を書き戻せず、退避に残したまま */
	pending: string[];
	/** 相手が削除したため、書き戻さず退避に残した */
	removedUpstream: string[];
}

/** 通知 1 件の文言のキーと、埋め込む値 */
export interface SkipNoticeLine {
	key: MessageKey;
	values: Record<string, string | number>;
}

/** 件のパスを、表示用の名前の並びにする（先頭の数件と、残りの件数） */
export function summarizeNames(paths: string[], limit = NOTICE_NAME_LIMIT) {
	const normalized = paths.map((path) => path.replaceAll('\\', '/'));
	return {
		names: normalized.slice(0, limit).join('、'),
		more: Math.max(0, normalized.length - limit)
	};
}

/**
 * 通知の文言（何が起きたか／データは無事か／次の行動 を 1 つの文にしたもの）を、件数の分だけ返す。
 * 何も起きなければ空。どちらもあれば、両方の文を返す。
 */
export function skipNoticeLines(notice: SkipNoticeInput): SkipNoticeLine[] {
	const lines: SkipNoticeLine[] = [];
	if (notice.pending.length > 0) {
		const { names, more } = summarizeNames(notice.pending);
		lines.push({
			key: 'skip.notice_pending',
			values: { names, count: notice.pending.length, more }
		});
	}
	if (notice.removedUpstream.length > 0) {
		const { names, more } = summarizeNames(notice.removedUpstream);
		lines.push({
			key: 'skip.notice_removed',
			values: { names, count: notice.removedUpstream.length, more }
		});
	}
	return lines;
}

/** 記録 1 件の状態。無視中か、無視が外れているか */
export type SkipEntryStatus = 'ignored' | 'lost';

export function skipEntryStatus(entry: SkipWorktreeEntry): SkipEntryStatus {
	return entry.ignored ? 'ignored' : 'lost';
}

/** 一覧に出すパス（区切りを `/` にそろえる） */
export function displayPath(path: string): string {
	return path.replaceAll('\\', '/');
}
