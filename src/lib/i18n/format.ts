import { currentLocale, t, type Locale } from './index';

// 言語ごとの Intl のフォーマッタ（言語が変わると、対応する方を使う）
const LOCALE_TAGS: Record<Locale, string> = { ja: 'ja-JP', en: 'en-US' };

function createFormatters(locale: Locale) {
	const tag = LOCALE_TAGS[locale];
	// 日本語は 24 時間表記、英語は 12 時間表記（AM / PM）
	const hour12 = locale === 'en';
	return {
		dateTime: new Intl.DateTimeFormat(tag, {
			month: 'numeric',
			day: 'numeric',
			hour: '2-digit',
			minute: '2-digit',
			hour12
		}),
		time: new Intl.DateTimeFormat(tag, {
			hour: '2-digit',
			minute: '2-digit',
			hour12
		}),
		date: new Intl.DateTimeFormat(tag, { month: 'numeric', day: 'numeric' })
	};
}

const FORMATTERS: Record<Locale, ReturnType<typeof createFormatters>> = {
	ja: createFormatters('ja'),
	en: createFormatters('en')
};

function formatters() {
	return FORMATTERS[currentLocale()];
}

export function formatDateTime(date: Date): string {
	return formatters().dateTime.format(date);
}

export function formatTime(date: Date): string {
	return formatters().time.format(date);
}

export function formatDate(date: Date): string {
	return formatters().date.format(date);
}

export function formatRelative(date: Date, now: Date = new Date()): string {
	const minutes = Math.floor((now.getTime() - date.getTime()) / 60_000);
	if (minutes < 1) return t('time.just_now');
	if (minutes < 60) return t('time.minutes_ago', { n: minutes });
	const hours = Math.floor(minutes / 60);
	if (hours < 24) return t('time.hours_ago', { n: hours });
	const days = Math.floor(hours / 24);
	if (days < 7) return t('time.days_ago', { n: days });
	return formatDate(date);
}

export function formatBytes(bytes: number): string {
	if (bytes < 1024) return `${bytes} B`;
	if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
	if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
	return `${(bytes / 1024 / 1024 / 1024).toFixed(1)} GB`;
}
