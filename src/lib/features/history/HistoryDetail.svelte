<script lang="ts">
	import { FolderTree, RotateCcw } from '@lucide/svelte';
	import { t } from '#lib/i18n/index.js';
	import { formatDateTime } from '#lib/i18n/format.js';
	import { api } from '#lib/api/index.js';
	import type { SavePoint } from '#lib/api/types.js';
	import Button from '#lib/components/Button.svelte';
	import FileActionRow from '#lib/components/FileActionRow.svelte';
	import Skeleton from '#lib/components/Skeleton.svelte';
	import FileTree from './FileTree.svelte';
	import { buildFileTree } from './file-tree';
	import { useFilesAt, usePointFiles } from './queries';

	interface Props {
		projectId: string;
		point: SavePoint | null;
		isNow: boolean;
		oncompare: (path: string) => void;
		onrestore: (path: string | null) => void;
		/** この時点の版を読み取り専用で開く */
		onopenat: (path: string) => void;
	}

	let { projectId, point, isNow, oncompare, onrestore, onopenat }: Props = $props();

	const files = usePointFiles(
		() => projectId,
		() => point?.id ?? null
	);
	let showAll = $state(false);

	$effect(() => {
		void point?.id;
		showAll = false;
	});

	const allFiles = useFilesAt(
		() => projectId,
		() => point?.id ?? null,
		() => showAll
	);
	const tree = $derived(buildFileTree(allFiles.data ?? []));
</script>

<div class="flex flex-direction:column flex:1 min-w:0 min-h:0 overflow-y:auto">
	{#if isNow}
		<div class="flex align-items:center justify-content:center flex:1 p:6">
			<p class="m:0 type-body fg:fg-muted">{t('history.now_hint')}</p>
		</div>
	{:else if !point}
		<div class="flex align-items:center justify-content:center flex:1 p:6">
			<p class="m:0 type-body fg:fg-muted">{t('history.select_hint')}</p>
		</div>
	{:else}
		<div class="px:6 pt:6 pb:4 bb:1px|solid|border">
			<p class="m:0 type-small fg:fg-muted">
				{formatDateTime(point.createdAt)}
				{#if point.pcName}
					・{point.pcName}
				{/if}
			</p>
			<h2 class="m:0 type-title">
				{point.kind === 'auto' ? t('timeline.auto_save') : point.message}
			</h2>
		</div>

		<section class="flex flex-direction:column" aria-labelledby="history-files-title">
			<h3 id="history-files-title" class="m:0 px:6 pt:4 pb:2 type-heading">
				{showAll
					? t('history.all_files', { count: allFiles.data?.length ?? 0 })
					: t('history.changed_files', { count: files.data?.length ?? 0 })}
			</h3>
			{#if showAll ? allFiles.isPending : files.isPending}
				<div class="flex flex-direction:column gap:2 px:6" aria-busy="true">
					<Skeleton class="h:32px" />
					<Skeleton class="h:32px" />
				</div>
			{:else if showAll}
				<FileTree
					nodes={tree}
					canRestore={api.capabilities.restoreFile}
					{onopenat}
					onrestore={(path) => onrestore(path)}
				/>
			{:else}
				<ul class="m:0 p:0 list-style:none">
					{#each files.data ?? [] as file (file.path)}
						<li class="px:2">
							<!-- この時点で削除されたファイルは、この時点の版が無いため開けない -->
							<FileActionRow
								filename={file.path}
								restoreLabel={t('file_action.restore_point')}
								oncompare={() => oncompare(file.path)}
								onrestore={api.capabilities.restoreFile ? () => onrestore(file.path) : undefined}
								onopenat={file.type === 'deleted' ? undefined : () => onopenat(file.path)}
							/>
						</li>
					{/each}
				</ul>
			{/if}
		</section>

		<div class="flex gap:3 px:6 py:6">
			<Button variant="secondary" onclick={() => (showAll = !showAll)}>
				<FolderTree size={16} aria-hidden="true" />
				{showAll ? t('history.show_changed') : t('timeline.view_all_files')}
			</Button>
			<Button variant="secondary" onclick={() => onrestore(null)}>
				<RotateCcw size={16} aria-hidden="true" />
				{t('timeline.restore_all')}
			</Button>
		</div>
	{/if}
</div>
