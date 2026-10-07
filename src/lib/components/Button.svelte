<script lang="ts">
	import type { Snippet } from 'svelte';
	import Spinner from './Spinner.svelte';

	interface Props {
		variant?: 'primary' | 'secondary' | 'ghost' | 'danger';
		size?: 'sm' | 'md';
		disabled?: boolean;
		loading?: boolean;
		type?: 'button' | 'submit' | 'reset';
		title?: string;
		ariaLabel?: string;
		class?: string;
		children?: Snippet;
		onclick?: (event: MouseEvent) => void;
	}

	let {
		variant = 'primary',
		size = 'md',
		disabled = false,
		loading = false,
		type = 'button',
		title,
		ariaLabel,
		class: className = '',
		children,
		onclick
	}: Props = $props();

	const base =
		'inline-flex align-items:center justify-content:center gap:2 r:md font-weight:500 b:0 cursor:pointer user-select:none white-space:nowrap transition:background-color|var(--duration-fast) opacity:.5:disabled cursor:not-allowed:disabled';

	const variants = {
		primary: 'bg:accent fg:accent-fg filter:brightness(.92):hover filter:brightness(.84):active',
		secondary: 'bg:bg-raised fg:fg b:1px|solid|border-strong bg:bg-subtle:hover bg:border:active',
		ghost: 'bg:transparent fg:fg bg:bg-subtle:hover bg:border:active',
		danger:
			'bg:state-danger fg:accent-fg filter:brightness(.92):hover filter:brightness(.84):active'
	};

	const sizes = {
		sm: 'px:3 py:1 type-small',
		md: 'px:4 py:2 type-body'
	};

	const classes = $derived(`${base} ${variants[variant]} ${sizes[size]} ${className}`);
</script>

<button
	{type}
	{title}
	aria-label={ariaLabel}
	aria-busy={loading || undefined}
	disabled={disabled || loading}
	{onclick}
	class={classes}
>
	{#if loading}
		<Spinner size="sm" />
	{/if}
	{@render children?.()}
</button>
