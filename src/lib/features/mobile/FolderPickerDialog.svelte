<script lang="ts">
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import Folder from '@lucide/svelte/icons/folder';
	import { t } from '#lib/i18n/index.js';
	import Button from '#lib/components/Button.svelte';
	import Dialog from '#lib/components/Dialog.svelte';
	import Spinner from '#lib/components/Spinner.svelte';
	import { breadcrumbsOf, parentPathOf } from './breadcrumb';
	import ErrorNotice from './ErrorNotice.svelte';
	import { sortEntries } from './listing';
	import { nav } from './nav.svelte';
	import { useDirListing } from './queries';

	interface Props {
		owner: string;
		name: string;
		/** 最初に開くフォルダ（追加先の現在値） */
		initial: string;
		onchoose: (path: string) => void;
		onclose: () => void;
	}

	let { owner, name, initial, onchoose, onclose }: Props = $props();

	/** 閲覧中のフォルダ。未操作のあいだは最初のフォルダ */
	let browse = $state<string | null>(null);
	const current = $derived(browse ?? initial);

	const listing = useDirListing(
		() => owner,
		() => name,
		() => current
	);
	const folders = $derived(
		listing.data ? sortEntries(listing.data.entries).filter((entry) => entry.kind === 'dir') : []
	);
	const crumbs = $derived(breadcrumbsOf(current));
</script>

<Dialog open title={t('mobile.upload.folder_title')} {onclose}>
	<div class="flex flex-direction:column gap:3">
		<nav
			aria-label={t('mobile.files.breadcrumb')}
			class="flex flex-flow:row|wrap align-items:center gap:1 type-small"
		>
			<button
				type="button"
				onclick={() => (browse = '')}
				class="min-h:44px px:2 b:0 r:md bg:transparent fg:accent cursor:pointer"
				>{t('mobile.files.root')}</button
			>
			{#each crumbs as crumb (crumb.path)}
				<ChevronRight size={14} aria-hidden="true" class="fg:fg-faint" />
				<button
					type="button"
					onclick={() => (browse = crumb.path)}
					class="min-h:44px px:2 b:0 r:md bg:transparent fg:accent cursor:pointer"
					>{crumb.label}</button
				>
			{/each}
		</nav>

		{#if listing.isPending}
			<div class="flex justify-content:center p:4"><Spinner /></div>
		{:else if listing.isError}
			<ErrorNotice
				error={listing.error}
				onretry={() => void listing.refetch()}
				onlogin={() => (nav.needsLogin = true)}
			/>
		{:else}
			{#if current !== ''}
				<button
					type="button"
					onclick={() => (browse = parentPathOf(current))}
					class="flex align-items:center gap:2 min-h:44px px:2 b:0 r:md bg:transparent fg:fg-muted cursor:pointer bg:bg-subtle:hover text-align:left"
					>{t('mobile.upload.folder_up')}</button
				>
			{/if}
			{#if folders.length === 0}
				<p class="m:0 type-small fg:fg-muted">{t('mobile.upload.folder_none')}</p>
			{:else}
				<ul class="list-style:none m:0 p:0 max-h:50vh overflow:auto">
					{#each folders as folder (folder.path)}
						<li>
							<button
								type="button"
								onclick={() => (browse = folder.path)}
								class="flex w:100% align-items:center gap:2 min-h:44px px:2 b:0 r:md bg:transparent fg:fg cursor:pointer bg:bg-subtle:hover text-align:left"
							>
								<Folder size={18} aria-hidden="true" class="fg:fg-muted" />
								<span
									class="flex:1 min-w:0 overflow:hidden text-overflow:ellipsis white-space:nowrap"
									>{folder.name}</span
								>
							</button>
						</li>
					{/each}
				</ul>
			{/if}
		{/if}

		<div class="flex gap:2">
			<Button class="min-h:44px" onclick={() => onchoose(current)}
				>{t('mobile.upload.folder_choose')}</Button
			>
		</div>
	</div>
</Dialog>
