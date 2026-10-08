<script lang="ts">
	import { Clock, CloudOff, Folder, User, Users } from '@lucide/svelte';
	import { t } from '#lib/i18n/index.js';
	import { formatRelative } from '#lib/i18n/format.js';
	import type { Project } from '#lib/api/types.js';
	import Badge from './Badge.svelte';
	import Button from './Button.svelte';
	import { cardStatus, needsAttention, needsConnection } from './project-card';

	interface Props {
		project: Project;
		syncing?: boolean;
		onclick?: () => void;
		/** GitHub に接続していないプロジェクトの「接続する」。指定しなければ案内の文言だけを出す */
		onconnect?: () => void;
		class?: string;
	}

	let { project, syncing = false, onclick, onconnect, class: className = '' }: Props = $props();

	const status = $derived(cardStatus(project));
	const unconnected = $derived(needsConnection(project));
	const badge = $derived.by(() => {
		if (syncing) return { variant: 'sync' as const, label: t('badge.syncing') };
		switch (status) {
			case 'folder_missing':
				return { variant: 'danger' as const, label: t('badge.folder_missing') };
			case 'conflict':
			case 'interrupted':
				return { variant: 'danger' as const, label: t('badge.danger') };
			case 'unsaved':
				return {
					variant: 'unsaved' as const,
					label: t('badge.unsaved', { count: project.unsavedCount })
				};
			case 'fetch_pending':
			case 'push_pending':
				return { variant: 'sync' as const, label: t('badge.sync_pending') };
			default:
				return { variant: 'saved' as const, label: t('badge.saved') };
		}
	});
	const frame = $derived(
		needsAttention(project)
			? 'b:1px|solid|state-danger'
			: 'b:1px|solid|border b:1px|solid|accent:hover'
	);
</script>

<!-- カード全体はプロジェクトを開くボタン。「接続する」は別のボタンなので、入れ子にならないよう兄弟に置く -->
<div class={`flex flex-direction:column w:100% r:md bg:bg-raised fg:fg ${frame} ${className}`}>
	<button
		type="button"
		{onclick}
		class="flex flex-direction:column gap:3 w:100% p:4 r:md bg:transparent fg:fg text-align:left cursor:pointer b:0"
	>
		<span class="flex align-items:center gap:2 min-w:0">
			<Folder size={20} class="fg:fg-muted flex-shrink:0" aria-hidden="true" />
			<span class="type-heading overflow:hidden text-overflow:ellipsis white-space:nowrap">
				{project.name}
			</span>
		</span>
		<span class="type-small fg:fg-muted overflow:hidden text-overflow:ellipsis white-space:nowrap">
			{project.path}
		</span>
		<span class="flex align-items:center flex-flow:row|wrap gap:2">
			<Badge variant={badge.variant} label={badge.label} />
			<span class="inline-flex align-items:center gap:1 type-small fg:fg-muted white-space:nowrap">
				<Clock size={14} aria-hidden="true" />
				{project.lastSavedAt
					? t('project_card.last_saved', { when: formatRelative(project.lastSavedAt) })
					: t('project_card.never_saved')}
			</span>
			<span class="inline-flex align-items:center gap:1 type-small fg:fg-muted white-space:nowrap">
				{#if project.ownerKind === 'org'}
					<Users size={14} aria-hidden="true" />
					{project.ownerName}
				{:else}
					<User size={14} aria-hidden="true" />
					{t('add_project.owner_personal')}
				{/if}
			</span>
		</span>
	</button>
	{#if unconnected}
		<div class="flex align-items:center justify-content:space-between gap:2 px:4 pb:3">
			<span class="inline-flex align-items:center gap:1 type-small fg:fg-muted min-w:0">
				<CloudOff size={14} class="flex-shrink:0" aria-hidden="true" />
				{t('project_card.not_connected')}
			</span>
			{#if onconnect}
				<Button size="sm" variant="secondary" onclick={onconnect}>{t('header.connect')}</Button>
			{/if}
		</div>
	{/if}
</div>
