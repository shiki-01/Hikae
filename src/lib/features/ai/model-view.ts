// AI モデルの表示と操作の判定（純関数）。文言は i18n キーで返し、画面で t() に通す。
import type {
	AiModel,
	AiScopeMode,
	AiUnavailableReason,
	CustomGgufReason
} from '#lib/api/types.js';
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

/** 段階の名前（軽量・標準）の文言キー。段階 1 は軽量、段階 2 以上は標準（設計 10.2） */
export function stageLevelKey(stage: number): MessageKey {
	return stage <= 1 ? 'ai.level_light' : 'ai.level_standard';
}

/** 段階 1 の説明の文言キー（段階 1 のときだけ） */
export function stageNoteKey(stage: number): MessageKey | null {
	return stage === 1 ? 'ai.stage_light_note' : null;
}

/**
 * AI に渡す範囲の注意（段階 1 のモデルで「テキスト差分まで」のときだけ、「ファイル名まで」を勧める）。
 * 勧めるだけで、利用者が選ぶことは妨げない。
 */
export function scopeHintKey(stage: number | null, scope: AiScopeMode): MessageKey | null {
	return stage === 1 && scope === 'text-diff' ? 'ai.scope_light_hint' : null;
}

/** カスタム GGUF の検査の理由の文言キー */
export function customGgufMessageKey(reason: CustomGgufReason): MessageKey {
	return `ai.custom_reason.${reason}` as MessageKey;
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

/**
 * 行に出す進捗。一覧がダウンロード中と示すときだけ出す（終了の通知を受け損ねても、進捗が残らないように）。
 * 通知（イベント）の値を優先し、無ければ一覧の取得時点の途中のバイト数から出す
 * （画面を再読み込みした直後など、通知をまだ受けていないとき）。
 */
export function downloadProgress(
	model: Pick<AiModel, 'downloading' | 'partialBytes' | 'sizeBytes'>,
	notified: { received: number; total: number } | null
): { received: number; total: number } | null {
	if (!model.downloading) return null;
	return notified ?? { received: model.partialBytes, total: model.sizeBytes };
}

/**
 * モデルの行に出す推奨のバッジの文言キー（`null` は出さない）。
 * この PC のおすすめ（総メモリと導入の可否で選ぶ）を優先し、無ければ段階の推奨（カタログの既定）を出す。
 */
export function recommendationKey(
	model: Pick<AiModel, 'recommended' | 'recommendedForThisPc'>
): MessageKey | null {
	if (model.recommendedForThisPc) return 'ai.recommended_for_pc';
	return model.recommended ? 'ai.recommended' : null;
}
