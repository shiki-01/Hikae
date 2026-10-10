import { t } from '#lib/i18n/index.js';
import { skipNoticeLines, type SkipNoticeInput } from './skip-model';

/**
 * 「今後の変更だけ無視する」の再適用の結果を、画面の文にする（設計書 4.8）。
 * 件の名前は先頭の数件だけを出し、残りは件数で示す。
 */
export function describeSkipNotice(notice: SkipNoticeInput): string[] {
	return skipNoticeLines(notice).map((line) => {
		const values: Record<string, string | number> = { ...line.values };
		const more = Number(values.more ?? 0);
		values.names =
			more > 0
				? t('skip.names_more', { names: String(values.names), count: more })
				: String(values.names);
		return t(line.key, values);
	});
}
