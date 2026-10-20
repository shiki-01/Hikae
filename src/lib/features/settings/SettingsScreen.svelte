<script lang="ts">
	import { page } from '$app/state';
	import ArrowLeft from '@lucide/svelte/icons/arrow-left';
	import { t, type MessageKey } from '#lib/i18n/index.js';
	import type { AppSettings, SettingKey } from '#lib/api/types.js';
	import Badge from '#lib/components/Badge.svelte';
	import Button from '#lib/components/Button.svelte';
	import Dialog from '#lib/components/Dialog.svelte';
	import IconButton from '#lib/components/IconButton.svelte';
	import Select from '#lib/components/Select.svelte';
	import AiSection from '#lib/features/ai/AiSection.svelte';
	import AccountSection from './AccountSection.svelte';
	import CloudBackupNote from './CloudBackupNote.svelte';
	import ExtensionsSection from './ExtensionsSection.svelte';
	import IgnoreSection from './IgnoreSection.svelte';
	import NumberSetting from './NumberSetting.svelte';
	import Skeleton from '#lib/components/Skeleton.svelte';
	import Tabs from '#lib/components/Tabs.svelte';
	import Toggle from '#lib/components/Toggle.svelte';
	import { useProject } from '#lib/features/projects/queries.js';
	import { goBackFromSettings } from '#lib/features/projects/relogin.js';
	import { useSettings, useUpdateSettings } from './queries';
	import {
		ADVANCED_ROWS,
		SETTING_ROWS,
		confirmationFor,
		isOverridden,
		isSettingsTab,
		optionValue,
		type SettingConfirmation,
		type SettingRow,
		type SettingValue
	} from './settings-model';

	/** 設定画面を開いたプロジェクト。無ければ「このプロジェクトだけ変更」は出さない */
	const contextProjectId = $derived(page.url.searchParams.get('project'));
	const contextProject = useProject(() => contextProjectId ?? '');

	let scope = $state<'all' | 'project'>('all');
	const activeProjectId = $derived(scope === 'project' ? contextProjectId : null);

	const settings = useSettings(() => activeProjectId);
	const update = useUpdateSettings(() => activeProjectId);
	// 詳細設定は全体の設定だけを読み書きする（プロジェクトの上書きを作らない）
	const globalSettings = useSettings(() => null);
	const globalUpdate = useUpdateSettings(() => null);

	// 他の画面から開くときは、?tab= で最初に開くタブを指定できる
	const requestedTab = page.url.searchParams.get('tab');
	let selected = $state(requestedTab && isSettingsTab(requestedTab) ? requestedTab : 'general');
	// 確認ダイアログで取りやめたとき、見た目だけ先に切り替わった部品を元の値で作り直す
	let nonce = $state(0);
	let pending = $state<{
		row: SettingRow;
		value: SettingValue;
		confirmation: SettingConfirmation;
	} | null>(null);

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

	const notes: Record<string, MessageKey> = {
		ignore: 'settings.note_ignore'
	};

	function change(current: AppSettings, row: SettingRow, value: SettingValue) {
		const confirmation = confirmationFor(row.key, current, value);
		if (confirmation) {
			pending = { row, value, confirmation };
			return;
		}
		apply(row, value);
	}

	function apply(row: SettingRow, value: SettingValue) {
		const patch = { [row.key]: value } as Partial<AppSettings>;
		// 全体の設定だけの項目（言語など）は、プロジェクトを開いていても全体へ書く
		if (row.globalOnly) globalUpdate.mutate(patch);
		else update.mutate(patch);
	}

	function confirmPending() {
		if (pending) apply(pending.row, pending.value);
		pending = null;
	}

	function cancelPending() {
		pending = null;
		nonce += 1;
	}

	function back() {
		void goBackFromSettings(page.url.search);
	}
</script>

<div class="h:100vh overflow:hidden flex flex-direction:column bg:bg fg:fg">
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

				{#if scope === 'project' && contextProject.data}
					<div
						class="flex align-items:center justify-content:space-between gap:3 p:3 r:md bg:accent-subtle"
						role="status"
					>
						<span class="type-body"
							>{t('settings.scope_project', { name: contextProject.data.name })}</span
						>
						<Button size="sm" variant="secondary" onclick={() => (scope = 'all')}>
							{t('settings.scope_all')}
						</Button>
					</div>
				{/if}

				{#if id === 'general' && scope === 'all'}
					<AccountSection />
				{/if}

				{#if isSettingsTab(id)}
					{#if settings.isPending}
						<Skeleton class="h:48px" />
						<Skeleton class="h:48px" />
					{:else if settings.data}
						{@const values = settings.data.settings}
						{@const overridden = settings.data.overridden}
						<ul class="m:0 p:0 list-style:none flex flex-direction:column">
							{#each SETTING_ROWS[id] as row (row.key)}
								{@render settingRow(row, values, overridden)}
							{/each}
						</ul>
					{/if}
					{#if id === 'auto_save' && contextProjectId}
						<CloudBackupNote projectId={contextProjectId} />
					{/if}
				{:else if id === 'advanced'}
					<details class="b:1px|solid|border r:md p:3">
						<summary class="type-body cursor:pointer">{t('settings.advanced_toggle')}</summary>
						{#if globalSettings.data}
							{@const globalValues = globalSettings.data.settings}
							<ul class="m:0 mt:3 p:0 list-style:none flex flex-direction:column">
								{#each ADVANCED_ROWS as row (row.key)}
									<li
										class="flex align-items:center justify-content:space-between gap:6 py:3 bb:1px|solid|border"
									>
										<div class="flex flex-direction:column gap:1 min-w:0">
											<span class="type-body font-weight:500">{t(row.label)}</span>
											<span class="type-small fg:fg-muted">{t(row.text)}</span>
										</div>
										<Toggle
											label={t(row.label)}
											checked={globalValues[row.key] as boolean}
											onchange={(checked) => globalUpdate.mutate({ [row.key]: checked })}
										/>
									</li>
								{/each}
							</ul>
						{:else}
							<Skeleton class="h:48px mt:3" />
						{/if}
					</details>
				{:else if id === 'ai'}
					<AiSection />
				{:else if id === 'ignore' && contextProjectId}
					<IgnoreSection projectId={contextProjectId} />
				{:else if id === 'extensions'}
					<ExtensionsSection />
				{:else if notes[id]}
					<p class="m:0 type-body fg:fg-muted">{t(notes[id])}</p>
				{/if}
			</div>
		{/snippet}
	</Tabs>
</div>

{#snippet settingRow(row: SettingRow, values: AppSettings, overridden: SettingKey[])}
	<li class="flex align-items:center justify-content:space-between gap:6 py:3 bb:1px|solid|border">
		<div class="flex flex-direction:column gap:1 min-w:0">
			<span class="type-body font-weight:500">{t(row.label)}</span>
			<span class="type-small fg:fg-muted">{t(row.text)}</span>
			{#if scope === 'project' && isOverridden(overridden, row.key)}
				<Badge variant="sync" label={t('settings.overridden')} class="align-self:flex-start" />
			{:else if scope === 'all' && contextProjectId && !row.globalOnly}
				<button
					type="button"
					class="align-self:flex-start p:0 b:0 bg:transparent fg:accent type-small cursor:pointer text-decoration:underline"
					onclick={() => (scope = 'project')}
				>
					{t('settings.project_only')}
				</button>
			{/if}
		</div>
		{#key `${activeProjectId}-${nonce}`}
			{#if row.type === 'toggle'}
				<Toggle
					label={t(row.label)}
					checked={values[row.key] as boolean}
					onchange={(checked) => change(values, row, checked)}
				/>
			{:else if row.type === 'number'}
				<NumberSetting
					{row}
					value={values[row.key]}
					onchange={(next) => change(values, row, next)}
				/>
			{:else}
				<Select
					class="w:200px flex-shrink:0"
					ariaLabel={t(row.label)}
					value={String(values[row.key])}
					options={row.options.map((option) => ({
						value: String(option.value),
						label: t(option.label)
					}))}
					onchange={(picked) => {
						const value = optionValue(row, picked);
						if (value !== undefined) change(values, row, value);
					}}
				/>
			{/if}
		{/key}
	</li>
{/snippet}

<Dialog
	open={pending !== null}
	variant="warning"
	title={pending ? t(pending.confirmation.title) : ''}
	description={pending ? t(pending.confirmation.text) : undefined}
	onclose={cancelPending}
>
	{#snippet actions()}
		<Button variant="secondary" onclick={cancelPending}>{t('settings.confirm_cancel')}</Button>
		<Button onclick={confirmPending}>{t('settings.confirm_apply')}</Button>
	{/snippet}
</Dialog>
