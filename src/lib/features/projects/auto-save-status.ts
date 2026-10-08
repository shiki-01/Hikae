import { t } from '#lib/i18n/index.js';
import { formatRelative } from '#lib/i18n/format.js';

/** 自動保存の待ち時間（秒）を、「30 秒」「2 分」のように読みやすくする */
export function formatDelay(secs: number): string {
	if (secs < 60) return t('autosave.duration_seconds', { n: secs });
	if (secs < 3600) {
		const minutes = Math.floor(secs / 60);
		const rest = secs % 60;
		return rest === 0
			? t('autosave.duration_minutes', { n: minutes })
			: t('autosave.duration_minutes_seconds', { n: minutes, count: rest });
	}
	if (secs % 3600 === 0) return t('autosave.duration_hours', { n: secs / 3600 });
	return t('autosave.duration_minutes', { n: Math.floor(secs / 60) });
}

/** 画面の上部に出す自動保存の説明 */
export type AutoSaveView =
	| { kind: 'off' }
	| {
			kind: 'on';
			/** 「ファイルを変更して 2 分操作しないと、自動で控えを残します。」 */
			rule: string;
			/** 「最後の自動保存: 3 分前」。まだ無いときは「まだ自動保存はありません」 */
			last: string;
	  };

export function autoSaveView(
	enabled: boolean,
	delaySecs: number,
	lastAt: Date | null,
	now: Date
): AutoSaveView {
	if (!enabled) return { kind: 'off' };
	return {
		kind: 'on',
		rule: t('autosave.rule', { when: formatDelay(delaySecs) }),
		last: lastAt ? t('autosave.last', { when: formatRelative(lastAt, now) }) : t('autosave.none')
	};
}
