// AI モデルの表示と操作の判定（純関数）。文言は i18n キーで返し、画面で t() に通す。
import type { AiModel, AiUnavailableReason } from '#lib/api/types.js';
import type { MessageKey, MessageParams } from '#lib/i18n/index.js';

const GB = 1024 ** 3;
const MB = 1024 ** 2;

/** 容量の表示。1 GB 以上は小数 1 桁（末尾の .0 は付けない）の GB、それ未満は MB */
export function formatBytes(bytes: number): string {
	if (bytes >= GB) {
		return `${(bytes / GB).toFixed(1).replace(/\.0$/, '')} GB`;
	}
	return `${Math.round(bytes / MB)} MB`;
}

/** 受信量の割合（0〜100 の整数）。総量が分からないときは 0 */
export function progressPercent(received: number, total: number): number {
	if (!(total > 0)) return 0;
	return Math.max(0, Math.min(100, Math.round((received / total) * 100)));
}

/** 導入できない理由の文言（キーと差し込む値） */
export function unavailableMessage(reason: AiUnavailableReason): {
	key: MessageKey;
	params: MessageParams;
} {
	if (reason.kind === 'memory') {
		return {
			key: 'ai.unavailable_memory',
			params: { needed: formatBytes(reason.needed), have: formatBytes(reason.have) }
		};
	}
	return {
		key: 'ai.unavailable_disk',
		params: { needed: formatBytes(reason.needed), have: formatBytes(reason.have) }
	};
}

/** モデルの行の操作 */
export type ModelAction = 'download' | 'cancel' | 'remove' | 'use';

/** モデルの行に出すボタン。`true` のものだけ出す */
export interface ModelButtons {
	download: boolean;
	resume: boolean;
	cancel: boolean;
	use: boolean;
	remove: boolean;
}

/**
 * ボタンの組み合わせ。
 * - ダウンロード中は「中止」だけ
 * - 導入できない（PC の条件を満たさない）ものは、取得も開始しない
 * - 導入済みは「使う」（選ばれていなければ）と「削除」
 * - 途中まで取得したものは「再開」と「削除」（途中の分を消す）
 */
export function modelButtons(model: AiModel, selected: boolean): ModelButtons {
	if (model.downloading) {
		return { download: false, resume: false, cancel: true, use: false, remove: false };
	}
	const canStart = model.available;
	return {
		download: canStart && model.status === 'missing',
		resume: canStart && model.status === 'partial',
		cancel: false,
		use: model.status === 'ready' && !selected,
		remove: model.status !== 'missing'
	};
}
