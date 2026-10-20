<script lang="ts">
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import { api } from '#lib/api/index.js';
	import type { ExtensionInfo } from '#lib/api/types.js';
	import { t } from '#lib/i18n/index.js';
	import { reportError } from '#lib/features/notifications/store.svelte.js';
	import Badge from '#lib/components/Badge.svelte';
	import Button from '#lib/components/Button.svelte';
	import Dialog from '#lib/components/Dialog.svelte';
	import IconButton from '#lib/components/IconButton.svelte';
	import Skeleton from '#lib/components/Skeleton.svelte';
	import Toggle from '#lib/components/Toggle.svelte';
	import { formatFileTypes } from './extension-model';
	import { useInstallExtension, useRemoveExtension, useSetExtensionEnabled } from './mutations';
	import { useExtensions, useSettings, useUpdateSettings } from './queries';

	// 拡張機能は全体の設定（プロジェクトごとの上書きは無い）
	const extensions = useExtensions();
	const settings = useSettings(() => null);
	const update = useUpdateSettings(() => null);
	const setEnabled = useSetExtensionEnabled();
	const install = useInstallExtension();
	const remove = useRemoveExtension();

	const devMode = $derived(settings.data?.settings.extensionDevMode ?? false);

	/** 開発者モードのスイッチを確認なしで戻す（取りやめたとき、見た目だけ先に変わった部品を作り直す） */
	let nonce = $state(0);
	let confirmDevOpen = $state(false);
	let removeTarget = $state<ExtensionInfo | null>(null);
	let picking = $state(false);

	function changeDevMode(checked: boolean) {
		if (checked) {
			// オンにするときだけ、署名のない拡張の危険を確認する
			confirmDevOpen = true;
			return;
		}
		update.mutate({ extensionDevMode: false });
	}

	function confirmDevMode() {
		confirmDevOpen = false;
		update.mutate({ extensionDevMode: true });
	}

	function cancelDevMode() {
		confirmDevOpen = false;
		nonce += 1;
	}

	async function addFromFolder() {
		picking = true;
		try {
			const folder = await api.pickFolder();
			if (folder !== null) install.mutate(folder);
		} catch (error) {
			reportError(error);
		} finally {
			picking = false;
		}
	}

	function confirmRemove() {
		const target = removeTarget;
		removeTarget = null;
		if (target) remove.mutate({ id: target.id, name: target.name });
	}
</script>

<div class="flex flex-direction:column gap:4">
	<p class="m:0 type-body fg:fg-muted">{t('extensions.description')}</p>
	<p class="m:0 type-small fg:fg-muted" role="note">{t('extensions.restart_note')}</p>

	{#if extensions.isPending}
		<Skeleton class="h:48px" />
	{:else if extensions.data && extensions.data.length > 0}
		<ul
			class="m:0 p:0 list-style:none flex flex-direction:column"
			aria-label={t('extensions.list_label')}
		>
			{#each extensions.data as ext (ext.id)}
				{@const formats = formatFileTypes(ext.fileTypes, t('extensions.file_type_any'))}
				<li
					class="flex align-items:center justify-content:space-between gap:3 py:3 bb:1px|solid|border"
				>
					<div class="flex flex-direction:column gap:1 min-w:0">
						<div class="flex align-items:center gap:2 flex-wrap:wrap">
							<span class="type-body font-weight:500 overflow-wrap:anywhere">{ext.name}</span>
							<Badge
								variant="neutral"
								label={ext.builtin ? t('extensions.badge_builtin') : t('extensions.badge_added')}
							/>
						</div>
						<span class="type-small fg:fg-muted"
							>{t('extensions.version', { version: ext.version })}</span
						>
						<span class="type-small fg:fg-muted">
							{formats ? t('extensions.file_types', { formats }) : t('extensions.file_types_none')}
						</span>
						{#if ext.builtin}
							<span class="type-small fg:fg-muted">{t('extensions.builtin_locked')}</span>
						{/if}
					</div>
					<div class="flex align-items:center gap:2 flex-shrink:0">
						{#if devMode && !ext.builtin}
							<IconButton
								label={t('extensions.remove_label', { name: ext.name })}
								onclick={() => (removeTarget = ext)}
							>
								<Trash2 size={18} aria-hidden="true" />
							</IconButton>
						{/if}
						{#key `${ext.id}-${ext.enabled}`}
							<Toggle
								label={t('extensions.enable_label', { name: ext.name })}
								checked={ext.enabled}
								disabled={ext.builtin || setEnabled.isPending}
								onchange={(checked) => setEnabled.mutate({ id: ext.id, enabled: checked })}
							/>
						{/key}
					</div>
				</li>
			{/each}
		</ul>
	{:else}
		<p class="m:0 type-body fg:fg-muted">{t('extensions.empty')}</p>
	{/if}

	<section class="flex flex-direction:column gap:2 mt:4">
		<h3 class="m:0 type-heading">{t('extensions.dev_section_title')}</h3>
		<p class="m:0 type-small fg:fg-muted">{t('extensions.dev_description')}</p>
		{#if settings.isPending}
			<Skeleton class="h:48px" />
		{:else}
			<div
				class="flex align-items:center justify-content:space-between gap:6 py:3 bb:1px|solid|border"
			>
				<span class="type-body">{t('extensions.dev_toggle')}</span>
				{#key `${devMode}-${nonce}`}
					<Toggle
						label={t('extensions.dev_toggle')}
						checked={devMode}
						disabled={update.isPending}
						onchange={changeDevMode}
					/>
				{/key}
			</div>
		{/if}
		{#if devMode}
			<div class="flex justify-content:flex-end">
				<Button size="sm" disabled={picking || install.isPending} onclick={addFromFolder}>
					{t('extensions.add_folder')}
				</Button>
			</div>
		{/if}
	</section>
</div>

<Dialog
	open={confirmDevOpen}
	variant="warning"
	title={t('extensions.dev_confirm_title')}
	description={t('extensions.dev_confirm_text')}
	onclose={cancelDevMode}
>
	{#snippet actions()}
		<Button variant="secondary" onclick={cancelDevMode}>{t('settings.confirm_cancel')}</Button>
		<Button onclick={confirmDevMode}>{t('extensions.dev_confirm_apply')}</Button>
	{/snippet}
</Dialog>

<Dialog
	open={removeTarget !== null}
	variant="warning"
	title={removeTarget ? t('extensions.remove_title', { name: removeTarget.name }) : ''}
	description={t('extensions.remove_text')}
	onclose={() => (removeTarget = null)}
>
	{#snippet actions()}
		<Button variant="secondary" onclick={() => (removeTarget = null)}>
			{t('settings.confirm_cancel')}
		</Button>
		<Button onclick={confirmRemove}>{t('extensions.remove_confirm')}</Button>
	{/snippet}
</Dialog>
