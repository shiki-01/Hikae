import { t } from './index';

const LOCALE = 'ja-JP';

const dateTimeFormat = new Intl.DateTimeFormat(LOCALE, {
	month: 'numeric',
	day: 'numeric',
	hour: '2-digit',
	minute: '2-digit',
	hour12: false
});

const timeFormat = new Intl.DateTimeFormat(LOCALE, {
	hour: '2-digit',
	minute: '2-digit',
	hour12: false
});

const dateFormat = new Intl.DateTimeFormat(LOCALE, { month: 'numeric', day: 'numeric' });

export function formatDateTime(date: Date): string {
	return dateTimeFormat.format(date);
}

export function formatTime(date: Date): string {
	return timeFormat.format(date);
}

export function formatDate(date: Date): string {
	return dateFormat.format(date);
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
