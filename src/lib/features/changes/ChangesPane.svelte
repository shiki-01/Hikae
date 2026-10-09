<script lang="ts">
	import { t } from '#lib/i18n/index.js';
	import { formatRelative } from '#lib/i18n/format.js';
	import type { Change, ProjectTree } from '#lib/api/types.js';
	import ChangeListItem from '#lib/components/ChangeListItem.svelte';
	import DropZone from '#lib/components/DropZone.svelte';
	import SegmentedControl from '#lib/components/SegmentedControl.svelte';
	import Skeleton from '#lib/components/Skeleton.svelte';
	import { actionsFor } from './change-actions';
	import ProjectFileTree from './ProjectFileTree.svelte';
	import type { ChangesView } from './view';

	interface Props {
		changes: Change[];
		loading: boolean;
		selectedPath: string | null;
		lastSavedAt: Date | null;
		dragging: boolean;
		tooLarge: boolean;
		/** 「変更のみ」か「すべてのファイル」か */
		view: ChangesView;
		/** 「すべてのファイル」の一覧。取得中は null */
		tree: ProjectTree | null;
		treeLoading: boolean;
		/** ファイルをドラッグ中の位置にあるフォルダ（「すべてのファイル」のときだけ）。追加先として強調する */
		dropFolder?: string | null;
		onviewchange: (view: ChangesView) => void;
		onselect: (path: string) => void;
		/** 新規ファイルの行の「元に戻す（作成しない）」 */
		ondiscard: (path: string) => void;
		onfiles: (files: File[]) => void;
		onpick?: () => void;
	}

	let {
		changes,
		loading,
		selectedPath,
		lastSavedAt,
		dragging,
		tooLarge,
		view,
		tree,
		treeLoading,
		dropFolder = null,
		onviewchange,
		onselect,
		ondiscard,
		onfiles,
		onpick
	}: Props = $props();
</script>

<div class="flex flex-direction:column flex:1 min-h:0">
	<div class="px:3 py:2 bb:1px|solid|border flex-shrink:0">
		<SegmentedControl
			value={view}
			ariaLabel={t('changes.view_label')}
			options={[
				{ value: 'changes', label: t('changes.view_changes') },
				{ value: 'tree', label: t('changes.view_tree') }
			]}
			onchange={(value) => onviewchange(value === 'tree' ? 'tree' : 'changes')}
		/>
	</div>
	<div class="flex:1 min-h:0 overflow-y:auto">
		{#if view === 'tree'}
			{#if treeLoading || tree === null}
				<div class="flex flex-direction:column gap:3 p:3" aria-busy="true">
					{#each [0, 1, 2, 3] as row (row)}
						<Skeleton class="h:32px" />
					{/each}
				</div>
			{:else}
				<p class="m:0 px:3 py:2 type-small fg:fg-muted">{t('changes.tree_drop_hint')}</p>
				<ProjectFileTree {tree} {selectedPath} {onselect} {dropFolder} />
			{/if}
		{:else if loading}
			<div class="flex flex-direction:column gap:3 p:3" aria-busy="true">
				{#each [0, 1, 2, 3] as row (row)}
					<Skeleton class="h:40px" />
				{/each}
			</div>
		{:else if changes.length === 0}
			<div class="flex flex-direction:column align-items:center gap:1 p:6 text-align:center">
				<p class="m:0 type-body font-weight:700">{t('changes.empty')}</p>
				{#if lastSavedAt}
					<p class="m:0 type-small fg:fg-muted">
						{t('changes.last_saved', { when: formatRelative(lastSavedAt) })}
					</p>
				{/if}
			</div>
		{:else}
			<ul class="m:0 p:0 list-style:none" aria-label={t('changes.list_label')}>
				{#each changes as change (change.id)}
					<li>
						<ChangeListItem
							{change}
							selected={selectedPath === change.path}
							onclick={() => onselect(change.path)}
							ondiscard={actionsFor(change).discard ? () => ondiscard(change.path) : undefined}
						/>
					</li>
				{/each}
			</ul>
		{/if}
	</div>
	<div class="p:3 flex-shrink:0 bt:1px|solid|border">
		<DropZone {dragging} {tooLarge} {onfiles} {onpick} />
	</div>
</div>
