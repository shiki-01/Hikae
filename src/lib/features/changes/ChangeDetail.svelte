<script lang="ts">
	import { untrack } from 'svelte';
	import { t } from '#lib/i18n/index.js';
	import type { Change, OpenTarget, ProjectFile } from '#lib/api/types.js';
	import Button from '#lib/components/Button.svelte';
	import FileActionRow from '#lib/components/FileActionRow.svelte';
	import FileDiffPane from '#lib/features/compare/FileDiffPane.svelte';
	import FilePreview from '#lib/features/compare/FilePreview.svelte';
	import { actionsFor } from './change-actions';
	import { defaultDetailView, type DetailView } from './detail-view';
	import type { ChangesView } from './view';

	interface Props {
		projectId: string;
		/** 選択中の変更。変更の無いファイルを選んだときは null */
		change: Change | null;
		/** 変更の無いファイル（「すべてのファイル」で選んだもの）。変更があるときは null */
		file?: ProjectFile | null;
		baseId: string;
		baseLabel: string;
		/** 左ペインの表示範囲。選択が変わったときの既定の表示（ファイル表示か、変更差分か）を決める */
		listView: ChangesView;
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
		listView,
		oncompare,
		onrestore,
		ondiscard,
		onopen
	}: Props = $props();

	const path = $derived(change?.path ?? file?.path ?? null);
	const actions = $derived(change ? actionsFor(change) : null);

	// 表示は「ファイル表示」か「変更差分」。選ぶファイルが変わるか、左ペインの表示範囲が変わったら既定に戻す
	let view = $state<DetailView>(untrack(() => defaultDetailView(listView)));
	$effect(() => {
		void path;
		view = defaultDetailView(listView);
	});
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
	{#if path}
		<div class="flex justify-content:flex-end px:4 py:2 bb:1px|solid|border flex-shrink:0">
			<Button
				size="sm"
				variant="secondary"
				onclick={() => (view = view === 'preview' ? 'diff' : 'preview')}
			>
				{view === 'preview' ? t('diff.view_to_diff') : t('diff.view_to_preview')}
			</Button>
		</div>
	{/if}
	{#if path === null}
		<p class="m:0 p:6 type-body fg:fg-muted text-align:center">{t('changes.select_hint')}</p>
	{:else if view === 'preview'}
		<!-- 現在のファイルの内容。削除されたファイルは「この時点にはこのファイルがありません」になる -->
		{#key `${path}|preview`}
			<FilePreview
				{projectId}
				rev="current"
				{path}
				label={t('timeline.now')}
				onopen={() => onopen('default')}
			/>
		{/key}
	{:else if change}
		{#key `${path}|diff`}
			<FileDiffPane
				{projectId}
				{path}
				from={baseId}
				to="current"
				fromLabel={baseLabel}
				toLabel={t('timeline.now')}
				onopen={() => onopen('default')}
			/>
		{/key}
	{:else}
		<p class="m:0 p:6 type-body fg:fg-muted text-align:center">{t('diff.no_changes')}</p>
	{/if}
</div>
