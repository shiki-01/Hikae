<script lang="ts">
	import { t } from '#lib/i18n/index.js';
	import type { Change, FileDiff, OpenTarget, ProjectFile } from '#lib/api/types.js';
	import DiffView from '#lib/components/DiffView.svelte';
	import FileActionRow from '#lib/components/FileActionRow.svelte';
	import { useCompare } from '#lib/features/compare/queries.js';
	import { fileKindOf } from '#lib/utils/file-kind.js';
	import { actionsFor } from './change-actions';

	interface Props {
		projectId: string;
		/** 選択中の変更。変更の無いファイルを選んだときは null */
		change: Change | null;
		/** 変更の無いファイル（「すべてのファイル」で選んだもの）。変更があるときは null */
		file?: ProjectFile | null;
		baseId: string;
		baseLabel: string;
		oncompare: () => void;
		onrestore: () => void;
		/** 新規ファイルを「元に戻す（作成しない）」 */
		ondiscard: () => void;
		onopen: (target: OpenTarget) => void;
	}

	let {
		projectId,
		change,
		file = null,
		baseId,
		baseLabel,
		oncompare,
		onrestore,
		ondiscard,
		onopen
	}: Props = $props();

	const diff = useCompare(
		() => projectId,
		() => change?.path ?? null,
		() => baseId,
		() => 'current'
	);

	const path = $derived(change?.path ?? file?.path ?? null);
	const actions = $derived(change ? actionsFor(change) : null);

	// 変更の無いファイルは差分が無いため、種類・サイズ・更新日時だけを見せる
	const fileInfo = $derived<FileDiff | null>(
		file && !change
			? {
					kind: 'info',
					fileKind: fileKindOf(file.path),
					sizeBytes: file.sizeBytes ?? 0,
					modifiedAt: file.modifiedAt
				}
			: null
	);
</script>

<div class="flex flex-direction:column flex:1 min-w:0 min-h:0">
	{#if change && actions}
		<FileActionRow
			filename={change.path}
			restoreLabel={actions.discard
				? t('file_action.discard_new')
				: t('file_action.restore_current')}
			openLabel={t('file_action.open_current')}
			{oncompare}
			onrestore={actions.discard ? ondiscard : onrestore}
			{onopen}
		/>
	{:else if file}
		<FileActionRow filename={file.path} openLabel={t('file_action.open_current')} {onopen} />
	{/if}
	<DiffView
		diff={change ? (diff.data ?? null) : fileInfo}
		loading={change !== null && diff.isPending}
		leftLabel={baseLabel}
		rightLabel={t('timeline.now')}
		infoText={fileInfo ? t('changes.tree_unchanged') : undefined}
		emptyText={path ? t('diff.select_file') : t('changes.select_hint')}
		onopen={() => onopen('default')}
	/>
</div>
