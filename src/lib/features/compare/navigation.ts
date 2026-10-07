import type { SelectOption } from '#lib/components/Select.svelte';
import type { SavePoint } from '#lib/api/types.js';
import { t } from '#lib/i18n/index.js';
import { formatDateTime } from '#lib/i18n/format.js';
import { latestManualPoint } from '#lib/components/timeline.js';

export type StepDirection = 1 | -1;

export function canStep(current: number | null, total: number, direction: StepDirection): boolean {
	if (total <= 0) return false;
	if (current === null) return true;
	return direction === 1 ? current < total - 1 : current > 0;
}

export function stepBlock(
	current: number | null,
	total: number,
	direction: StepDirection
): number | null {
	if (total <= 0) return null;
	if (current === null) return direction === 1 ? 0 : total - 1;
	return Math.min(total - 1, Math.max(0, current + direction));
}

export function swapPoints(from: string, to: string): { from: string; to: string } {
	return { from: to, to: from };
}

export function pointOptions(points: SavePoint[]): SelectOption[] {
	const options: SelectOption[] = [{ value: 'current', label: t('timeline.now') }];
	for (const point of [...points].sort((a, b) => b.createdAt.getTime() - a.createdAt.getTime())) {
		options.push({
			value: point.id,
			label: formatDateTime(point.createdAt),
			description: point.kind === 'auto' ? t('timeline.auto_save') : point.message
		});
	}
	return options;
}

export function defaultPoints(points: SavePoint[]): { from: string; to: string } {
	const latest = latestManualPoint(points);
	return { from: latest?.id ?? 'current', to: 'current' };
}
