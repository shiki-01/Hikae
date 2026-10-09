<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { FolderPlus, Plus, Settings } from '@lucide/svelte';
	import { t } from '#lib/i18n/index.js';
	import type { Project } from '#lib/api/types.js';
	import Button from '#lib/components/Button.svelte';
	import IconButton from '#lib/components/IconButton.svelte';
	import ProjectCard from '#lib/components/ProjectCard.svelte';
	import Skeleton from '#lib/components/Skeleton.svelte';
	import { AppError } from '#lib/api/errors.js';
	import { pushToast, reportError } from '#lib/features/notifications/store.svelte.js';
	import AddProjectDialog from './AddProjectDialog.svelte';
	import ConnectRemoteDialog from './ConnectRemoteDialog.svelte';
	import RemoveProjectDialog from './RemoveProjectDialog.svelte';
	import { useRelocateProject, useRemoveProject } from './mutations';
	import { useProjects, useSession } from './queries';
	import { sortProjects } from './sort';

	const projects = useProjects();
	const session = useSession();
	const relocate = useRelocateProject();
	const remove = useRemoveProject();
	// カードのメニューから「一覧から外す」を選んだプロジェクト（確認のダイアログを開いている間だけ入る）
	let removing = $state<Project | null>(null);
	const removeConfirmed = useRemoveProject(() => {
		if (removing)
			pushToast({ type: 'info', message: t('toast.project_removed', { name: removing.name }) });
		removing = null;
	});

	let adding = $state(false);
	// GitHub に接続するプロジェクト（ダイアログを開いている間だけ入る）
	let connecting = $state<Project | null>(null);

	const sorted = $derived(sortProjects(projects.data ?? []));

	$effect(() => {
		if (session.data && !session.data.onboarded) void goto(resolve('/welcome'));
	});

	function openProject(project: Project) {
		if (project.folderMissing) {
			reportError(new AppError('E11', `ENOENT: ${project.path}`, { name: project.name }), {
				onprimary: () => relocate.mutate(project.id),
				onsecondary: () => remove.mutate(project.id)
			});
			return;
		}
		void goto(`${resolve('/project')}?id=${encodeURIComponent(project.id)}`);
	}
</script>

<div class="h:100vh flex flex-direction:column bg:bg fg:fg">
	<header
		class="flex align-items:center justify-content:space-between h:56px px:4 flex-shrink:0 bg:bg-subtle bb:1px|solid|border"
	>
		<h1 class="m:0 type-title">{t('projects.title')}</h1>
		<div class="flex align-items:center gap:2">
			<Button size="sm" onclick={() => (adding = true)}>
				<Plus size={16} aria-hidden="true" />
				{t('projects.add')}
			</Button>
			<IconButton label={t('projects.settings')} onclick={() => goto(resolve('/settings'))}>
				<Settings size={20} aria-hidden="true" />
			</IconButton>
		</div>
	</header>

	<main class="flex:1 min-h:0 overflow-y:auto p:6">
		{#if projects.isPending}
			<div
				class="grid grid-template-columns:repeat(auto-fill,minmax(280px,1fr)) gap:4"
				aria-busy="true"
			>
				{#each [0, 1, 2] as card (card)}
					<Skeleton class="h:112px r:md" />
				{/each}
			</div>
		{:else if sorted.length === 0}
			<div class="flex flex-direction:column align-items:center gap:3 p:8 text-align:center">
				<FolderPlus size={48} class="fg:fg-faint" aria-hidden="true" />
				<h2 class="m:0 type-heading">{t('projects.empty')}</h2>
				<p class="m:0 type-body fg:fg-muted">{t('projects.empty_description')}</p>
				<Button onclick={() => (adding = true)}>
					<Plus size={16} aria-hidden="true" />
					{t('projects.empty')}
				</Button>
			</div>
		{:else}
			<ul
				class="m:0 p:0 list-style:none grid grid-template-columns:repeat(auto-fill,minmax(280px,1fr)) gap:4"
				aria-label={t('projects.list_label')}
			>
				{#each sorted as project (project.id)}
					<li>
						<ProjectCard
							{project}
							onclick={() => openProject(project)}
							onconnect={() => (connecting = project)}
							onremove={() => (removing = project)}
						/>
					</li>
				{/each}
			</ul>
		{/if}
	</main>
</div>

<AddProjectDialog
	open={adding}
	onclose={() => (adding = false)}
	onadded={(project) => goto(`${resolve('/project')}?id=${encodeURIComponent(project.id)}`)}
/>

<RemoveProjectDialog
	name={removing?.name ?? null}
	pending={removeConfirmed.isPending}
	onconfirm={() => removing && removeConfirmed.mutate(removing.id)}
	oncancel={() => (removing = null)}
/>

<ConnectRemoteDialog
	open={connecting !== null}
	projectId={connecting?.id ?? null}
	projectName={connecting?.name ?? ''}
	onclose={() => (connecting = null)}
/>
