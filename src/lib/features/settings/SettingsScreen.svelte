<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { ArrowLeft } from '@lucide/svelte';
	import { t, type MessageKey } from '#lib/i18n/index.js';
	import type { AppSettings } from '#lib/api/types.js';
	import IconButton from '#lib/components/IconButton.svelte';
	import Skeleton from '#lib/components/Skeleton.svelte';
	import Tabs from '#lib/components/Tabs.svelte';
	import Toggle from '#lib/components/Toggle.svelte';
	import { useSettings, useUpdateSettings } from './queries';

	const settings = useSettings();
	const update = useUpdateSettings();

	let selected = $state('general');

	const tabs: { id: string; label: MessageKey }[] = [
		{ id: 'general', label: 'settings.general' },
		{ id: 'fetch', label: 'settings.fetch' },
		{ id: 'push', label: 'settings.push' },
		{ id: 'auto_save', label: 'settings.auto_save' },
		{ id: 'ignore', label: 'settings.ignore' },
		{ id: 'ai', label: 'settings.ai' },
		{ id: 'extensions', label: 'settings.extensions' },
		{ id: 'advanced', label: 'settings.advanced' }
	];

	const toggles: Record<string, { key: keyof AppSettings; label: MessageKey; text: MessageKey }[]> =
		{
			general: [
				{
					key: 'autoSaveAfterRestore',
					label: 'settings.auto_save_after_restore',
					text: 'settings.auto_save_after_restore_text'
				}
			],
			fetch: [
				{
					key: 'autoSaveBeforeFetch',
					label: 'settings.auto_save_before_fetch',
					text: 'settings.auto_save_before_fetch_text'
				},
				{
					key: 'autoFetchOnLaunch',
					label: 'settings.auto_fetch_on_launch',
					text: 'settings.auto_fetch_on_launch_text'
				}
			],
			push: [
				{
					key: 'autoUploadOnSave',
					label: 'settings.auto_upload_on_save',
					text: 'settings.auto_upload_on_save_text'
				}
			]
		};

	const notes: Record<string, MessageKey> = {
		auto_save: 'settings.note_auto_save',
		ignore: 'settings.note_ignore',
		ai: 'settings.note_ai',
		extensions: 'settings.note_extensions'
	};

	function back() {
		if (history.length > 1) history.back();
		else void goto(resolve('/'));
	}
</script>

<div class="h:100vh flex flex-direction:column bg:bg fg:fg">
	<header
		class="flex align-items:center gap:2 h:56px px:4 flex-shrink:0 bg:bg-subtle bb:1px|solid|border"
	>
		<IconButton label={t('settings.back')} onclick={back}>
			<ArrowLeft size={20} aria-hidden="true" />
		</IconButton>
		<h1 class="m:0 type-title">{t('settings.title')}</h1>
	</header>

	<Tabs
		bind:selected
		ariaLabel={t('settings.title')}
		tabs={tabs.map((tab) => ({ id: tab.id, label: t(tab.label) }))}
	>
		{#snippet panel(id)}
			<div class="max-w:640px flex flex-direction:column gap:4">
				<h2 class="m:0 type-heading">
					{t(tabs.find((tab) => tab.id === id)?.label ?? 'settings.general')}
				</h2>

				{#if toggles[id]}
					{#if settings.isPending}
						<Skeleton class="h:48px" />
						<Skeleton class="h:48px" />
					{:else if settings.data}
						{@const values = settings.data}
						<ul class="m:0 p:0 list-style:none flex flex-direction:column">
							{#each toggles[id] as row (row.key)}
								<li
									class="flex align-items:center justify-content:space-between gap:6 py:3 bb:1px|solid|border"
								>
									<div class="flex flex-direction:column">
										<span class="type-body font-weight:500">{t(row.label)}</span>
										<span class="type-small fg:fg-muted">{t(row.text)}</span>
									</div>
									<Toggle
										label={t(row.label)}
										checked={values[row.key]}
										onchange={(checked) => update.mutate({ [row.key]: checked })}
									/>
								</li>
							{/each}
						</ul>
					{/if}
				{:else if id === 'advanced'}
					<details class="b:1px|solid|border r:md p:3">
						<summary class="type-body cursor:pointer">{t('settings.advanced_toggle')}</summary>
						<p class="m:0 mt:3 type-body fg:fg-muted">{t('settings.advanced_empty')}</p>
					</details>
				{:else if notes[id]}
					<p class="m:0 type-body fg:fg-muted">{t(notes[id])}</p>
				{/if}
			</div>
		{/snippet}
	</Tabs>
</div>
