<script lang="ts">
	import { AlertTriangle, ArrowRight, Minus, Pencil, Plus } from '@lucide/svelte';
	import { t } from '#lib/i18n/index.js';
	import type { Change } from '#lib/api/types.js';

	interface Props {
		change: Change;
		selected?: boolean;
		onclick?: () => void;
		class?: string;
	}

	let { change, selected = false, onclick, class: className = '' }: Props = $props();

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

<button
	type="button"
	aria-current={selected ? 'true' : undefined}
	{onclick}
	class={`flex align-items:center gap:3 w:100% px:3 py:2 b:0 text-align:left cursor:pointer fg:fg ${
		selected ? 'bg:accent-subtle' : 'bg:transparent bg:bg-subtle:hover'
	} ${className}`}
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
