<script lang="ts">
	import { AlertTriangle, ArrowRight, Minus, Pencil, Plus, RotateCcw } from '@lucide/svelte';
	import { t } from '#lib/i18n/index.js';
	import type { Change } from '#lib/api/types.js';
	import IconButton from './IconButton.svelte';

	interface Props {
		change: Change;
		selected?: boolean;
		onclick?: () => void;
		/** 指定すると、行の右端に「元に戻す（作成しない）」を出す（新規ファイルの行だけに渡す） */
		ondiscard?: () => void;
		class?: string;
	}

	let { change, selected = false, onclick, ondiscard, class: className = '' }: Props = $props();

	const icons = { modified: Pencil, added: Plus, deleted: Minus, renamed: ArrowRight };
	const colors = {
		modified: 'fg:fg-muted',
		added: 'fg:state-saved',
		deleted: 'fg:state-danger',
		renamed: 'fg:fg-muted'
	};

	const Icon = $derived(change.isConflict ? AlertTriangle : icons[change.type]);
	const iconColor = $derived(change.isConflict ? 'fg:state-danger' : colors[change.type]);
	const separator = $derived(Math.max(change.path.lastIndexOf('/'), change.path.lastIndexOf('\\')));
	const filename = $derived(change.path.slice(separator + 1));
	const folder = $derived(separator >= 0 ? change.path.slice(0, separator) : '');
</script>

<div
	class={`flex align-items:center ${selected ? 'bg:accent-subtle' : 'bg:transparent bg:bg-subtle:hover'} ${className}`}
>
	<button
		type="button"
		aria-current={selected ? 'true' : undefined}
		{onclick}
		class="flex align-items:center gap:3 flex:1 min-w:0 px:3 py:2 b:0 bg:transparent text-align:left cursor:pointer fg:fg"
	>
		<Icon size={16} class={`${iconColor} flex-shrink:0`} aria-hidden="true" />
		<span class="flex:1 min-w:0 flex flex-direction:column">
			<span class="type-body overflow:hidden text-overflow:ellipsis white-space:nowrap"
				>{filename}</span
			>
			{#if folder}
				<span
					class="type-small fg:fg-muted overflow:hidden text-overflow:ellipsis white-space:nowrap"
				>
					{folder}
				</span>
			{/if}
		</span>
		<span
			class={`type-small flex-shrink:0 ${change.isConflict ? 'fg:state-danger font-weight:700' : 'fg:fg-muted'}`}
		>
			{change.isConflict ? t('change.conflict') : t(`change.${change.type}`)}
		</span>
	</button>
	{#if ondiscard}
		<!-- ボタンの中にボタンを入れられないため、行の外に並べる -->
		<IconButton
			label={t('file_action.discard_new')}
			variant="subtle"
			tooltip="left"
			class="mr:2 flex-shrink:0"
			onclick={ondiscard}
		>
			<RotateCcw size={16} aria-hidden="true" />
		</IconButton>
	{/if}
</div>
