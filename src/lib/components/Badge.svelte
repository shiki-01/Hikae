<script lang="ts">
	import { AlertTriangle, Check, CloudUpload, Pencil } from '@lucide/svelte';

	type Variant = 'saved' | 'unsaved' | 'sync' | 'danger' | 'neutral';

	interface Props {
		variant?: Variant;
		count?: number;
		label?: string;
		class?: string;
	}

	let { variant = 'neutral', count, label, class: className = '' }: Props = $props();

	const borders: Record<Variant, string> = {
		saved: 'b:1px|solid|state-saved',
		unsaved: 'b:1px|solid|state-unsaved',
		sync: 'b:1px|solid|state-sync',
		danger: 'b:1px|solid|state-danger',
		neutral: 'b:1px|solid|border'
	};

	const icons = { saved: Check, unsaved: Pencil, sync: CloudUpload, danger: AlertTriangle };
	const iconColors: Record<Variant, string> = {
		saved: 'fg:state-saved',
		unsaved: 'fg:state-unsaved',
		sync: 'fg:state-sync',
		danger: 'fg:state-danger',
		neutral: 'fg:fg-muted'
	};

	const Icon = $derived(variant === 'neutral' ? null : icons[variant]);
</script>

<span
	class={`inline-flex align-items:center gap:1 px:2 r:sm bg:bg fg:fg type-small font-weight:500 white-space:nowrap ${borders[variant]} ${className}`}
>
	{#if Icon}
		<Icon size={12} class={iconColors[variant]} aria-hidden="true" />
	{/if}
	{#if count !== undefined}
		{count}
	{:else if label}
		{label}
	{/if}
</span>
