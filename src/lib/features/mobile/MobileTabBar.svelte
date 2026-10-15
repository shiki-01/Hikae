<script lang="ts">
	import Folder from '@lucide/svelte/icons/folder';
	import Settings from '@lucide/svelte/icons/settings';
	import Upload from '@lucide/svelte/icons/upload';
	import type { Component } from 'svelte';
	import { t, type MessageKey } from '#lib/i18n/index.js';
	import { nav, setTab, type MobileTab } from './nav.svelte';

	/** 下端のタブ（docs/design.md 14.7）。文言はすべて i18n キー経由 */
	const TABS: { id: MobileTab; label: MessageKey; icon: Component }[] = [
		{ id: 'files', label: 'mobile.tab.files', icon: Folder },
		{ id: 'upload', label: 'mobile.tab.upload', icon: Upload },
		{ id: 'settings', label: 'mobile.tab.settings', icon: Settings }
	];
</script>

<nav
	aria-label={t('mobile.tab.label')}
	class="flex flex-shrink:0 bt:1px|solid|border bg:bg"
	style="padding-bottom: env(safe-area-inset-bottom)"
>
	{#each TABS as tab (tab.id)}
		{@const active = nav.tab === tab.id}
		<button
			type="button"
			aria-current={active ? 'page' : undefined}
			onclick={() => setTab(tab.id)}
			class={`flex flex:1 flex-direction:column align-items:center justify-content:center gap:1 min-h:56px b:0 bg:transparent cursor:pointer ${
				active ? 'fg:accent font-weight:700' : 'fg:fg-muted'
			}`}
		>
			<tab.icon size={22} aria-hidden="true" />
			<span class="type-small">{t(tab.label)}</span>
		</button>
	{/each}
</nav>
