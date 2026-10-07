<script lang="ts">
	import { FolderTree, RotateCcw } from '@lucide/svelte';
	import { t } from '#lib/i18n/index.js';
	import { formatDateTime } from '#lib/i18n/format.js';
	import type { OpenTarget, PointFile, SavePoint } from '#lib/api/types.js';
	import Button from '#lib/components/Button.svelte';
	import FileActionRow from '#lib/components/FileActionRow.svelte';
	import Skeleton from '#lib/components/Skeleton.svelte';
	import { usePointFiles } from './queries';

	interface Props {
		projectId: string;
		point: SavePoint | null;
		isNow: boolean;
		oncompare: (path: string) => void;
		onrestore: (path: string | null) => void;
		onopen: (path: string, target: OpenTarget) => void;
	}

	let { projectId, point, isNow, oncompare, onrestore, onopen }: Props = $props();

	const files = usePointFiles(
		() => projectId,
		() => point?.id ?? null
	);
	let showAll = $state(false);

	$effect(() => {
		void point?.id;
		showAll = false;
	});

	interface Group {
		folder: string;
		files: PointFile[];
	}

	const groups = $derived.by<Group[]>(() => {
		const byFolder: Record<string, PointFile[]> = {};
		for (const file of files.data ?? []) {
			const slash = Math.max(file.path.lastIndexOf('/'), file.path.lastIndexOf('\\'));
			const folder = slash >= 0 ? file.path.slice(0, slash) : '';
			(byFolder[folder] ??= []).push(file);
		}
		return Object.entries(byFolder)
			.sort(([a], [b]) => a.localeCompare(b))
			.map(([folder, list]) => ({ folder, files: list }));
	});

	function baseName(path: string): string {
		return path.slice(Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\')) + 1);
	}
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
					? t('history.all_files', { count: files.data?.length ?? 0 })
					: t('history.changed_files', { count: files.data?.length ?? 0 })}
			</h3>
			{#if files.isPending}
				<div class="flex flex-direction:column gap:2 px:6" aria-busy="true">
					<Skeleton class="h:32px" />
					<Skeleton class="h:32px" />
				</div>
			{:else if showAll}
				{#each groups as group (group.folder)}
					<div class="px:6 py:2">
						{#if group.folder}
							<p class="m:0 mb:1 type-small fg:fg-muted">{group.folder}</p>
						{/if}
						<ul class="m:0 p:0 list-style:none">
							{#each group.files as file (file.path)}
								<li class="flex align-items:center justify-content:space-between gap:3 py:1">
									<span class="type-body">{baseName(file.path)}</span>
									<span class="flex gap:2">
										<Button size="sm" variant="ghost" onclick={() => onopen(file.path, 'default')}>
											{t('file_action.open')}
										</Button>
										<Button size="sm" variant="secondary" onclick={() => onrestore(file.path)}>
											{t('file_action.restore_point')}
										</Button>
									</span>
								</li>
							{/each}
						</ul>
					</div>
				{/each}
			{:else}
				<ul class="m:0 p:0 list-style:none">
					{#each files.data ?? [] as file (file.path)}
						<li>
							<FileActionRow
								filename={file.path}
								restoreLabel={t('file_action.restore_point')}
								oncompare={() => oncompare(file.path)}
								onrestore={() => onrestore(file.path)}
								onopen={(target) => onopen(file.path, target)}
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
