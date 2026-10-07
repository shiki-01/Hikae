<script lang="ts">
	import type { Snippet } from 'svelte';
	import Spinner from './Spinner.svelte';
	import Tooltip from './Tooltip.svelte';

	interface Props {
		label: string;
		variant?: 'ghost' | 'subtle';
		disabled?: boolean;
		loading?: boolean;
		tooltip?: 'top' | 'bottom' | 'left' | 'right';
		class?: string;
		children: Snippet;
		onclick?: (event: MouseEvent) => void;
	}

	let {
		label,
		variant = 'ghost',
		disabled = false,
		loading = false,
		tooltip = 'bottom',
		class: className = '',
		children,
		onclick
	}: Props = $props();

	const variants = {
		ghost: 'fg:fg bg:transparent bg:bg-subtle:hover bg:border:active',
		subtle: 'fg:fg-muted bg:transparent fg:fg:hover bg:bg-subtle:hover'
	};
</script>

<Tooltip text={label} position={tooltip}>
	<button
		type="button"
		aria-label={label}
		aria-busy={loading || undefined}
		disabled={disabled || loading}
		{onclick}
		class={`inline-flex align-items:center justify-content:center size:32px r:md b:0 cursor:pointer transition:background-color|var(--duration-fast) opacity:.5:disabled cursor:not-allowed:disabled ${variants[variant]} ${className}`}
	>
		{#if loading}
			<Spinner size="sm" />
		{:else}
			{@render children()}
		{/if}
	</button>
</Tooltip>
