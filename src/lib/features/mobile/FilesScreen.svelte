<script lang="ts">
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import Ellipsis from '@lucide/svelte/icons/ellipsis';
	import File from '@lucide/svelte/icons/file';
	import FileSpreadsheet from '@lucide/svelte/icons/file-spreadsheet';
	import FileText from '@lucide/svelte/icons/file-text';
	import Folder from '@lucide/svelte/icons/folder';
	import ImageIcon from '@lucide/svelte/icons/image';
	import Presentation from '@lucide/svelte/icons/presentation';
	import type { Component } from 'svelte';
	import { t } from '#lib/i18n/index.js';
	import { formatBytes, formatDateTime } from '#lib/i18n/format.js';
	import type { MobileDirEntry, MobileVersion } from '#lib/api/mobile-types.js';
	import Button from '#lib/components/Button.svelte';
	import Dialog from '#lib/components/Dialog.svelte';
	import Spinner from '#lib/components/Spinner.svelte';
	import { breadcrumbsOf } from './breadcrumb';
	import { downloadEntry } from './mobile-actions';
	import ErrorNotice from './ErrorNotice.svelte';
	import { extensionOf } from './mime';
	import { sortEntries } from './listing';
	import ScreenHeader from './ScreenHeader.svelte';
	import PreviewScreen from './PreviewScreen.svelte';
	import VersionsDialog from './VersionsDialog.svelte';
	import { closePreview, beginRepoChange, nav, openFolder, openPreview } from './nav.svelte';
	import { useDirListing } from './queries';

	const repo = $derived(nav.repo ?? { owner: '', name: '' });

	const listing = useDirListing(
		() => repo.owner,
		() => repo.name,
		() => nav.folder
	);

	const entries = $derived(listing.data ? sortEntries(listing.data.entries) : []);
	const crumbs = $derived(breadcrumbsOf(nav.folder));

	/** 「⋯」で開いた項目（操作の一覧を出している） */
	let actionTarget = $state<MobileDirEntry | null>(null);
	/** 過去の版の一覧を出しているファイル */
	let versionsTarget = $state<MobileDirEntry | null>(null);

	/** 種類ごとのアイコン（拡張子から決める） */
	function iconOf(entry: MobileDirEntry): Component {
		if (entry.kind === 'dir') return Folder;
		const ext = extensionOf(entry.name);
		if (['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg'].includes(ext)) return ImageIcon;
		if (['xlsx', 'csv'].includes(ext)) return FileSpreadsheet;
		if (ext === 'pptx') return Presentation;
		if (['pdf', 'docx', 'txt', 'md'].includes(ext)) return FileText;
		return File;
	}

	function previewEntry(entry: MobileDirEntry): void {
		openPreview({ path: entry.path, name: entry.name, sha: entry.sha, versionLabel: null });
	}

	function openVersionPreview(path: string, name: string, version: MobileVersion): void {
		versionsTarget = null;
		openPreview({
			path,
			name,
			sha: version.sha,
			versionLabel: formatDateTime(new Date(version.date))
		});
	}
</script>

{#if nav.preview}
	<PreviewScreen target={nav.preview} onback={closePreview} />
{:else}
	{#snippet headerActions()}
		<Button variant="ghost" size="sm" class="min-h:44px" onclick={beginRepoChange}
			>{t('mobile.files.change_repo')}</Button
		>
	{/snippet}
	<ScreenHeader title={`${repo.owner} / ${repo.name}`} actions={headerActions} />

	<nav
		aria-label={t('mobile.files.breadcrumb')}
		class="flex flex-wrap:wrap align-items:center gap:1 px:3 py:1 bb:1px|solid|border type-small"
	>
		<button
			type="button"
			onclick={() => openFolder('')}
			aria-current={nav.folder === '' ? 'location' : undefined}
			class="min-h:44px px:2 b:0 r:md bg:transparent fg:accent cursor:pointer bg:bg-subtle:hover"
			>{t('mobile.files.root')}</button
		>
		{#each crumbs as crumb, index (crumb.path)}
			<ChevronRight size={14} aria-hidden="true" class="fg:fg-faint" />
			<button
				type="button"
				onclick={() => openFolder(crumb.path)}
				aria-current={index === crumbs.length - 1 ? 'location' : undefined}
				class="min-h:44px px:2 b:0 r:md bg:transparent fg:accent cursor:pointer bg:bg-subtle:hover"
				>{crumb.label}</button
			>
		{/each}
	</nav>

	<div class="flex flex-direction:column flex:1 min-h:0 overflow:auto">
		{#if listing.isPending}
			<div class="flex justify-content:center p:6"><Spinner /></div>
		{:else if listing.isError}
			<ErrorNotice
				error={listing.error}
				onretry={() => void listing.refetch()}
				onlogin={() => (nav.needsLogin = true)}
			/>
		{:else if listing.data}
			{#if listing.data.truncated}
				<p class="m:0 px:4 py:2 type-small fg:fg-muted">{t('mobile.files.truncated')}</p>
			{/if}
			{#if entries.length === 0}
				<p class="m:0 p:4 type-body fg:fg-muted">{t('mobile.files.empty')}</p>
			{:else}
				<ul class="list-style:none m:0 p:0">
					{#each entries as entry (entry.path)}
						{@const Icon = iconOf(entry)}
						<li class="flex align-items:center bb:1px|solid|border">
							<button
								type="button"
								onclick={() =>
									entry.kind === 'dir' ? openFolder(entry.path) : previewEntry(entry)}
								class="flex flex:1 min-w:0 align-items:center gap:3 min-h:44px px:4 py:2 b:0 bg:transparent fg:fg text-align:left cursor:pointer bg:bg-subtle:hover"
							>
								<Icon size={20} aria-hidden="true" class="fg:fg-muted flex-shrink:0" />
								<span class="flex flex-direction:column min-w:0 flex:1">
									<span class="type-body overflow:hidden text-overflow:ellipsis white-space:nowrap"
										>{entry.name}</span
									>
									{#if entry.kind === 'file'}
										<span class="type-small fg:fg-muted">{formatBytes(entry.size)}</span>
									{/if}
								</span>
								{#if entry.kind === 'dir'}
									<ChevronRight size={18} aria-hidden="true" class="fg:fg-faint flex-shrink:0" />
								{/if}
							</button>
							{#if entry.kind === 'file'}
								<button
									type="button"
									aria-label={t('mobile.files.more', { name: entry.name })}
									onclick={() => (actionTarget = entry)}
									class="inline-flex align-items:center justify-content:center min-w:44px min-h:44px mr:2 b:0 r:md bg:transparent fg:fg-muted cursor:pointer bg:bg-subtle:hover flex-shrink:0"
								>
									<Ellipsis size={20} aria-hidden="true" />
								</button>
							{/if}
						</li>
					{/each}
				</ul>
			{/if}
		{/if}
	</div>

	{#if actionTarget}
		{@const target = actionTarget}
		<Dialog open title={target.name} onclose={() => (actionTarget = null)}>
			<div class="flex flex-direction:column gap:2">
				<Button
					class="min-h:44px"
					onclick={() => void downloadEntry(repo.owner, repo.name, target.path, target.sha)}
					>{t('mobile.common.download')}</Button
				>
				<Button
					variant="secondary"
					class="min-h:44px"
					onclick={() => {
						versionsTarget = target;
						actionTarget = null;
					}}>{t('mobile.common.versions')}</Button
				>
			</div>
		</Dialog>
	{/if}

	{#if versionsTarget}
		{@const target = versionsTarget}
		<VersionsDialog
			owner={repo.owner}
			name={repo.name}
			path={target.path}
			fileName={target.name}
			onclose={() => (versionsTarget = null)}
			onview={(version) => openVersionPreview(target.path, target.name, version)}
			ondownload={(version) => void downloadEntry(repo.owner, repo.name, target.path, version.sha)}
		/>
	{/if}
{/if}
