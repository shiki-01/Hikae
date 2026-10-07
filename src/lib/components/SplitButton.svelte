<script lang="ts">
	import { ChevronDown } from '@lucide/svelte';
	import Menu, { type MenuEntry } from './Menu.svelte';
	import { t } from '#lib/i18n/index.js';

	interface Props {
		label: string;
		items: MenuEntry[];
		variant?: 'secondary' | 'ghost';
		size?: 'sm' | 'md';
		disabled?: boolean;
		onclick?: () => void;
		class?: string;
	}

	let {
		label,
		items,
		variant = 'secondary',
		size = 'sm',
		disabled = false,
		onclick,
		class: className = ''
	}: Props = $props();

	let open = $state(false);
	let host = $state<HTMLDivElement>();
	let toggle = $state<HTMLButtonElement>();

	const frame = $derived(variant === 'secondary' ? 'b:1px|solid|border-strong' : '');
	const look = $derived(
		variant === 'secondary'
			? 'bg:bg-raised fg:fg bg:bg-subtle:hover'
			: 'bg:transparent fg:fg bg:bg-subtle:hover'
	);
	const padding = $derived(size === 'sm' ? 'py:1 type-small' : 'py:2 type-body');
	const shared = $derived(
		`inline-flex align-items:center b:0 ${look} ${padding} cursor:pointer opacity:.5:disabled cursor:not-allowed:disabled`
	);

	function closeMenu(restoreFocus: boolean) {
		open = false;
		if (restoreFocus) toggle?.focus();
	}
</script>

<div bind:this={host} class={`position:relative inline-flex ${className}`}>
	<div class={`inline-flex r:md overflow:hidden ${frame}`}>
		<button
			type="button"
			{disabled}
			{onclick}
			class={`${shared} px:3 font-weight:500 white-space:nowrap`}
		>
			{label}
		</button>
		<span class="w:1px align-self:stretch bg:border-strong" aria-hidden="true"></span>
		<button
			bind:this={toggle}
			type="button"
			{disabled}
			aria-haspopup="menu"
			aria-expanded={open}
			aria-label={t('a11y.more_actions', { name: label })}
			onclick={() => (open = !open)}
			class={`${shared} px:2`}
		>
			<ChevronDown size={14} aria-hidden="true" />
		</button>
	</div>
	<Menu {items} {open} {label} align="end" ignore={host} onclose={closeMenu} />
</div>
