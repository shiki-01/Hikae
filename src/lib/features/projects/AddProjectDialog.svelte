<script lang="ts">
	import { CloudDownload, FolderOpen, FolderPlus } from '@lucide/svelte';
	import type { Component } from 'svelte';
	import { t, type MessageKey } from '#lib/i18n/index.js';
	import { api } from '#lib/api/index.js';
	import type { AddProjectMode, ClonePhase, Project } from '#lib/api/types.js';
	import Button from '#lib/components/Button.svelte';
	import Dialog from '#lib/components/Dialog.svelte';
	import ProgressBar from '#lib/components/ProgressBar.svelte';
	import Radio from '#lib/components/Radio.svelte';
	import Skeleton from '#lib/components/Skeleton.svelte';
	import TextField from '#lib/components/TextField.svelte';
	import { reportError } from '#lib/features/notifications/store.svelte.js';
	import { joinPath } from '#lib/utils/path.js';
	import {
		canSubmit,
		emptyForm,
		nameFromFolder,
		validateAddProject,
		type AddProjectForm
	} from './add-project';
	import { useAddProject } from './mutations';
	import { useOwners, useRemoteProjects } from './queries';
	import RemoteTargetFields from './RemoteTargetFields.svelte';

	interface Props {
		open: boolean;
		initialMode?: AddProjectMode | null;
		onclose: () => void;
		onadded?: (project: Project) => void;
	}

	let { open, initialMode = null, onclose, onadded }: Props = $props();

	const owners = useOwners();
	const ownerList = $derived(owners.data ?? []);
	const defaultOwner = $derived(ownerList.find((o) => o.canCreate)?.id ?? 'personal');
	// 保存先を選べない（未ログインなど）ときは、ローカルだけに登録する
	const cloudAvailable = $derived(!owners.isError);

	let step = $state<'choose' | 'form'>('choose');
	let form = $state<AddProjectForm>(emptyForm('existing', 'personal'));
	let submitted = $state(false);
	let search = $state('');
	let picking = $state(false);
	// 選んだ既存のフォルダが、プロジェクトにするには広すぎる（ホーム・ドライブのルートなど）
	let folderTooBroad = $state(false);
	let clonePhase = $state<ClonePhase | null>(null);

	const remote = useRemoteProjects(
		() => search,
		() => open && step === 'form' && form.mode === 'github'
	);

	const add = useAddProject(
		(project) => {
			onadded?.(project);
			onclose();
		},
		(phase) => (clonePhase = phase)
	);

	$effect(() => {
		if (!open) return;
		submitted = false;
		clonePhase = null;
		search = '';
		folderTooBroad = false;
		step = initialMode ? 'form' : 'choose';
		form = emptyForm(initialMode ?? 'existing', defaultOwner);
	});

	const errors = $derived(submitted ? validateAddProject(form, ownerList, cloudAvailable) : {});
	const remoteList = $derived(remote.data?.projects ?? []);
	const pickedRemote = $derived(remoteList.find((r) => r.id === form.remoteId));
	const cloneTarget = $derived(
		pickedRemote && form.folder ? joinPath(form.folder, pickedRemote.name) : ''
	);
	const cloneMessages = {
		preparing: 'add_project.clone_preparing',
		downloading: 'add_project.clone_downloading',
		finishing: 'add_project.clone_finishing',
		done: 'add_project.clone_done',
		failed: 'add_project.clone_failed'
	} as const satisfies Record<ClonePhase, MessageKey>;
	// 保存先の一覧に無い所有者のプロジェクトも、取りこぼさず並べる
	const groups = $derived(
		[...new Set(remoteList.map((r) => r.ownerId))].map((ownerId) => ({
			owner: ownerList.find((o) => o.id === ownerId) ?? {
				id: ownerId,
				name: ownerId,
				kind: 'org' as const,
				canCreate: false
			},
			items: remoteList.filter((r) => r.ownerId === ownerId)
		}))
	);

	const cards: { mode: AddProjectMode; icon: Component; title: MessageKey; text: MessageKey }[] = [
		{
			mode: 'existing',
			icon: FolderOpen,
			title: 'add_project.existing',
			text: 'add_project.existing_text'
		},
		{
			mode: 'github',
			icon: CloudDownload,
			title: 'add_project.from_github',
			text: 'add_project.from_github_text'
		},
		{ mode: 'new', icon: FolderPlus, title: 'add_project.new', text: 'add_project.new_text' }
	];

	function choose(mode: AddProjectMode) {
		form = emptyForm(mode, defaultOwner);
		submitted = false;
		folderTooBroad = false;
		step = 'form';
	}

	async function pickFolder() {
		picking = true;
		try {
			const folder = await api.pickFolder();
			if (folder === null) return;
			form.folder = folder;
			if (form.mode === 'existing' && !form.name.trim()) form.name = nameFromFolder(form.folder);
			// 既存のフォルダはそのものを見守るので、広すぎないかを登録の前に知らせる（バックエンドも拒否する）。
			// 取得・新規作成は、選んだフォルダの中に新しいフォルダを作るので対象外
			folderTooBroad =
				form.mode === 'existing' ? (await api.checkProjectFolder(folder)) !== null : false;
		} catch (error) {
			reportError(error);
		} finally {
			picking = false;
		}
	}

	function submit() {
		submitted = true;
		if (folderTooBroad || !canSubmit(form, ownerList, cloudAvailable)) return;
		const picked = pickedRemote;
		clonePhase = null;
		add.mutate({
			mode: form.mode,
			name: form.mode === 'github' ? (picked?.name ?? '') : form.name.trim(),
			folder: form.folder,
			ownerId: form.mode === 'github' ? (picked?.ownerId ?? form.ownerId) : form.ownerId,
			visibility: form.visibility,
			publicConfirmed: form.visibility === 'public' && form.publicConfirmed,
			connectCloud: cloudAvailable && form.mode !== 'github',
			remoteId: form.remoteId ?? undefined
		});
	}

	function back() {
		if (initialMode) onclose();
		else step = 'choose';
	}
</script>

<Dialog
	{open}
	variant="form"
	title={t('add_project.title')}
	description={step === 'choose' ? t('add_project.choose_hint') : undefined}
	busy={add.isPending}
	{onclose}
>
	{#if step === 'choose'}
		<div class="flex flex-direction:column gap:3">
			{#each cards as card (card.mode)}
				{@const Icon = card.icon}
				<button
					type="button"
					onclick={() => choose(card.mode)}
					class="flex align-items:center gap:4 p:4 r:md bg:bg-raised fg:fg b:1px|solid|border-strong b:1px|solid|accent:hover text-align:left cursor:pointer"
				>
					<span
						class="size:40px r:md bg:accent-subtle flex align-items:center justify-content:center flex-shrink:0"
						aria-hidden="true"
					>
						<Icon size={20} class="fg:accent" />
					</span>
					<span class="flex flex-direction:column">
						<span class="type-body font-weight:700">{t(card.title)}</span>
						<span class="type-small fg:fg-muted">{t(card.text)}</span>
					</span>
				</button>
			{/each}
		</div>
	{:else}
		<form
			id="add-project-form"
			class="flex flex-direction:column gap:4"
			onsubmit={(event) => {
				event.preventDefault();
				submit();
			}}
		>
			<h3 class="m:0 type-heading">
				{t(
					form.mode === 'existing'
						? 'add_project.existing'
						: form.mode === 'github'
							? 'add_project.from_github'
							: 'add_project.new'
				)}
			</h3>

			{#if form.mode === 'github'}
				<TextField
					bind:value={search}
					type="search"
					label={t('add_project.github_search')}
					placeholder={t('add_project.github_search_placeholder')}
				/>
				<div class="flex flex-direction:column gap:3" aria-live="polite">
					{#if remote.isPending}
						<Skeleton class="h:24px" />
						<Skeleton class="h:24px" />
					{:else if groups.length === 0}
						<p class="m:0 type-body fg:fg-muted">{t('add_project.github_empty')}</p>
					{:else}
						{#each groups as group (group.owner.id)}
							<fieldset class="m:0 p:0 b:0 flex flex-direction:column gap:2">
								<legend class="type-small fg:fg-muted mb:1">
									{group.owner.kind === 'org'
										? t('add_project.owner_org', { name: group.owner.name })
										: t('add_project.owner_personal_named', { name: group.owner.name })}
								</legend>
								{#each group.items as item (item.id)}
									<Radio
										name="remote-project"
										value={item.id}
										group={form.remoteId}
										label={item.name}
										onselect={(value) => (form.remoteId = value)}
									/>
								{/each}
							</fieldset>
						{/each}
					{/if}
					{#if remote.data?.truncated}
						<p class="m:0 type-small fg:fg-muted">{t('add_project.github_truncated')}</p>
					{/if}
					{#if errors.remote}
						<p class="m:0 type-small fg:state-danger" role="alert">{t(errors.remote)}</p>
					{/if}
				</div>
			{:else}
				<TextField
					bind:value={form.name}
					label={t('add_project.name')}
					error={errors.name ? t(errors.name) : undefined}
					maxLength={60}
				/>
			{/if}

			<div class="flex align-items:end gap:2">
				<TextField
					value={form.folder}
					readonly
					label={form.mode === 'github'
						? t('add_project.github_clone_folder')
						: form.mode === 'new'
							? t('add_project.folder_new')
							: t('add_project.folder')}
					placeholder={t('add_project.folder_placeholder')}
					error={errors.folder
						? t(errors.folder)
						: folderTooBroad
							? t('add_project.error_folder_too_broad')
							: undefined}
					class="flex:1"
				/>
				<Button variant="secondary" loading={picking} onclick={pickFolder}>
					{t('add_project.browse')}
				</Button>
			</div>

			{#if form.mode === 'github' && cloneTarget}
				<p class="m:0 type-small fg:fg-muted overflow-wrap:anywhere">
					{t('add_project.github_clone_target', { path: cloneTarget })}
				</p>
			{/if}

			{#if form.mode === 'github' && add.isPending}
				{@const phase = clonePhase ?? 'preparing'}
				<div class="flex flex-direction:column gap:2" aria-live="polite">
					<ProgressBar
						indeterminate={phase !== 'done'}
						value={100}
						state={phase === 'failed' ? 'failed' : phase === 'done' ? 'done' : 'running'}
						label={t('add_project.clone_label')}
					/>
					<p class="m:0 type-small fg:fg-muted">{t(cloneMessages[phase])}</p>
				</div>
			{/if}

			{#if form.mode !== 'github'}
				{#if cloudAvailable}
					<RemoteTargetFields
						owners={ownerList}
						loading={owners.isPending}
						bind:ownerId={form.ownerId}
						bind:visibility={form.visibility}
						bind:publicConfirmed={form.publicConfirmed}
						ownerError={errors.owner ? t(errors.owner) : undefined}
						confirmError={errors.confirm ? t(errors.confirm) : undefined}
					/>
				{:else}
					<p class="m:0 type-small fg:fg-muted" role="note">{t('add_project.local_only_hint')}</p>
				{/if}
			{/if}
		</form>
	{/if}

	{#snippet actions()}
		{#if step === 'form'}
			<Button variant="ghost" disabled={add.isPending} onclick={back}
				>{t('add_project.back')}</Button
			>
			<span class="flex:1"></span>
			<Button variant="secondary" disabled={add.isPending} onclick={onclose}>
				{t('add_project.cancel')}
			</Button>
			<Button loading={add.isPending} onclick={submit}>
				{t('add_project.button')}
			</Button>
		{:else}
			<Button variant="secondary" onclick={onclose}>{t('add_project.cancel')}</Button>
		{/if}
	{/snippet}
</Dialog>
