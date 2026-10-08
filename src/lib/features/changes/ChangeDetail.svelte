<script lang="ts">
	import { t } from '#lib/i18n/index.js';
	import type { Change, OpenTarget } from '#lib/api/types.js';
	import DiffView from '#lib/components/DiffView.svelte';
	import FileActionRow from '#lib/components/FileActionRow.svelte';
	import { useCompare } from '#lib/features/compare/queries.js';

	interface Props {
		projectId: string;
		change: Change | null;
		baseId: string;
		baseLabel: string;
		oncompare: () => void;
		onrestore: () => void;
		onopen: (target: OpenTarget) => void;
	}

	let { projectId, change, baseId, baseLabel, oncompare, onrestore, onopen }: Props = $props();

	const diff = useCompare(
		() => projectId,
		() => change?.path ?? null,
		() => baseId,
		() => 'current'
	);
</script>

<div class="flex flex-direction:column flex:1 min-w:0 min-h:0">
	{#if change}
		<FileActionRow
			filename={change.path}
			restoreLabel={t('file_action.restore_current')}
			openLabel={t('file_action.open_current')}
			{oncompare}
			{onrestore}
			{onopen}
		/>
	{/if}
	<DiffView
		diff={change ? (diff.data ?? null) : null}
		loading={change !== null && diff.isPending}
		leftLabel={baseLabel}
		rightLabel={t('timeline.now')}
		emptyText={change ? t('diff.select_file') : t('changes.select_hint')}
		onopen={() => onopen('default')}
	/>
</div>
