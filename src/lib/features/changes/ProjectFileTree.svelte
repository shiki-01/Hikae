<script lang="ts">
	import { t } from '#lib/i18n/index.js';
	import type { ProjectTree } from '#lib/api/types.js';
	import FileTree from '#lib/features/history/FileTree.svelte';
	import { buildFileTree } from '#lib/features/history/file-tree.js';

	interface Props {
		tree: ProjectTree;
		selectedPath: string | null;
		onselect: (path: string) => void;
	}

	let { tree, selectedPath, onselect }: Props = $props();

	const nodes = $derived(
		buildFileTree(
			tree.files.map((file) => ({
				path: file.path,
				size: file.sizeBytes,
				change: file.change,
				isConflict: file.isConflict
			}))
		)
	);
</script>

{#if tree.files.length === 0}
	<p class="m:0 p:6 type-body fg:fg-muted text-align:center">{t('changes.tree_empty')}</p>
{:else}
	<!-- 変更のあるフォルダは最初から開く。それ以外は、開いたときに中身を描画する -->
	<FileTree
		{nodes}
		{selectedPath}
		{onselect}
		defaultOpen={(folder) => folder.hasChange}
		label={t('changes.tree_label')}
		showSize
	/>
{/if}
{#if tree.truncated}
	<p class="m:0 px:4 py:3 type-small fg:fg-muted">
		{t('changes.tree_truncated', { count: tree.limit })}
	</p>
{/if}
