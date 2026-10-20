<script lang="ts">
	import Archive from '@lucide/svelte/icons/archive';
	import ArrowDownToLine from '@lucide/svelte/icons/arrow-down-to-line';
	import ChevronDown from '@lucide/svelte/icons/chevron-down';
	import Layers from '@lucide/svelte/icons/layers';
	import Plus from '@lucide/svelte/icons/plus';
	import { t } from '#lib/i18n/index.js';
	import { formatDateTime } from '#lib/i18n/format.js';
	import type { WorkCopy } from '#lib/api/types.js';
	import Button from '#lib/components/Button.svelte';
	import Dialog from '#lib/components/Dialog.svelte';
	import Menu, { type MenuEntry } from '#lib/components/Menu.svelte';
	import TextField from '#lib/components/TextField.svelte';
	import { useWorkCopies } from './queries';
	import { useWorkCopyActions } from './mutations';
	import {
		archivable,
		canAdopt,
		isInvalidDisplayName,
		normalizeDisplayName
	} from './work-copy-model';

	interface Props {
		projectId: string;
		/** 取り込みでぶつかりが残った。解消の画面を開く */
		onconflict: () => void;
		/** 未保存の変更を保存するため、変更タブを開く */
		onshowchanges: () => void;
	}

	let { projectId, onconflict, onshowchanges }: Props = $props();

	const works = useWorkCopies(() => projectId);
	const items = $derived(works.data ?? []);
	const current = $derived(items.find((w) => w.isCurrent) ?? null);

	let menuOpen = $state(false);
	let menuHost = $state<HTMLDivElement>();
	let nameOpen = $state(false);
	let adoptOpen = $state(false);
	let unsavedOpen = $state(false);
	let name = $state('');

	// 「別の案を試す」を開くたびに、名前の入力を空に戻す
	$effect(() => {
		if (nameOpen) name = '';
	});

	const work = useWorkCopyActions(() => projectId, {
		onCreated: () => (nameOpen = false),
		onUnsaved: () => {
			nameOpen = false;
			adoptOpen = false;
			unsavedOpen = true;
		},
		onConflict: () => {
			adoptOpen = false;
			onconflict();
		},
		onAdopted: () => (adoptOpen = false)
	});

	const nameValid = $derived(normalizeDisplayName(name) !== null);
	const nameInvalid = $derived(isInvalidDisplayName(name));
	const busy = $derived(work.create.isPending || work.adopt.isPending);

	/** 作業コピーの表示名。表示名が無いものは作成時刻から「無題の案（日時）」を補う */
	function labelOf(item: WorkCopy): string {
		if (item.isOriginal) return t('wc.original');
		if (item.displayName) return item.displayName;
		return item.createdAt
			? t('wc.untitled_at', { time: formatDateTime(item.createdAt) })
			: t('wc.untitled');
	}

	function menuLabelOf(item: WorkCopy): string {
		const label = labelOf(item);
		return item.isCurrent ? t('wc.current_label', { name: label }) : label;
	}

	const menuItems = $derived<MenuEntry[]>([
		...items.map<MenuEntry>((item) => ({
			label: menuLabelOf(item),
			onselect: () => {
				// いまの作業コピーを選んでも何もしない
				if (!item.isCurrent) work.switchTo.mutate(item.branch);
			}
		})),
		{ type: 'separator' },
		{ label: t('wc.new'), icon: Plus, onselect: () => (nameOpen = true) },
		...(canAdopt(items)
			? [
					{
						label: t('wc.adopt'),
						icon: ArrowDownToLine,
						onselect: () => (adoptOpen = true)
					}
				]
			: []),
		...archivable(items).map<MenuEntry>((item) => ({
			label: t('wc.archive', { name: labelOf(item) }),
			icon: Archive,
			onselect: () => work.archive.mutate({ branch: item.branch, archived: true })
		}))
	]);

	function submitName() {
		const value = normalizeDisplayName(name);
		if (value === null || busy) return;
		work.create.mutate(value);
	}

	function confirmAdopt() {
		if (!current || busy) return;
		work.adopt.mutate(current.branch);
	}
</script>

<div
	bind:this={menuHost}
	class="position:relative flex align-items:center gap:3 px:4 py:2 bb:1px|solid|border bg:bg-subtle flex-shrink:0"
>
	<Layers size={16} class="fg:fg-muted flex-shrink:0" aria-hidden="true" />
	<span class="type-small fg:fg-muted flex-shrink:0">{t('wc.label')}</span>
	<Button
		size="sm"
		variant="secondary"
		ariaExpanded={menuOpen}
		onclick={() => (menuOpen = !menuOpen)}
	>
		<span class="overflow:hidden text-overflow:ellipsis white-space:nowrap min-w:0">
			{current ? labelOf(current) : t('wc.label')}
		</span>
		<ChevronDown size={14} aria-hidden="true" />
	</Button>
	<Menu
		items={menuItems}
		open={menuOpen}
		label={t('wc.menu')}
		ignore={menuHost}
		onclose={() => (menuOpen = false)}
	/>
</div>

<Dialog
	open={nameOpen}
	variant="form"
	title={t('wc.new_title')}
	description={t('wc.new_description')}
	{busy}
	onclose={() => (nameOpen = false)}
>
	<TextField
		label={t('wc.name_label')}
		placeholder={t('wc.name_placeholder')}
		description={t('wc.name_hint')}
		error={nameInvalid ? t('wc.name_invalid') : undefined}
		bind:value={name}
		onkeydown={(event) => {
			if (event.key === 'Enter') submitName();
		}}
	/>

	{#snippet actions()}
		<Button variant="ghost" disabled={busy} onclick={() => (nameOpen = false)}>
			{t('wc.cancel')}
		</Button>
		<Button loading={work.create.isPending} disabled={!nameValid || busy} onclick={submitName}>
			{t('wc.name_submit')}
		</Button>
	{/snippet}
</Dialog>

<Dialog
	open={adoptOpen}
	variant="confirm"
	title={t('wc.adopt_title')}
	description={t('wc.adopt_description')}
	{busy}
	onclose={() => (adoptOpen = false)}
>
	{#snippet actions()}
		<Button variant="ghost" disabled={busy} onclick={() => (adoptOpen = false)}>
			{t('wc.cancel')}
		</Button>
		<Button loading={work.adopt.isPending} disabled={busy} onclick={confirmAdopt}>
			{t('wc.adopt_submit')}
		</Button>
	{/snippet}
</Dialog>

<Dialog
	open={unsavedOpen}
	variant="warning"
	title={t('wc.unsaved_title')}
	description={t('wc.unsaved_description')}
	onclose={() => (unsavedOpen = false)}
>
	<p class="m:0 type-body">{t('wc.unsaved_next')}</p>

	{#snippet actions()}
		<Button variant="ghost" onclick={() => (unsavedOpen = false)}>{t('wc.close')}</Button>
		<Button
			onclick={() => {
				unsavedOpen = false;
				onshowchanges();
			}}
		>
			{t('wc.show_changes')}
		</Button>
	{/snippet}
</Dialog>
