<script lang="ts">
	import { Check } from '@lucide/svelte';

	interface Props {
		checked?: boolean;
		label: string;
		disabled?: boolean;
		onchange?: (checked: boolean) => void;
		class?: string;
	}

	let {
		checked = $bindable(false),
		label,
		disabled = false,
		onchange,
		class: className = ''
	}: Props = $props();

	function toggle() {
		checked = !checked;
		onchange?.(checked);
	}
</script>

<button
	type="button"
	role="switch"
	aria-checked={checked}
	aria-label={label}
	{disabled}
	onclick={toggle}
	class={`position:relative inline-flex flex-shrink:0 w:44px h:24px r:full b:0 p:0 cursor:pointer transition:background-color|var(--duration-fast) opacity:.5:disabled cursor:not-allowed:disabled ${checked ? 'bg:accent' : 'bg:border-strong'} ${className}`}
>
	<span
		class={`position:absolute top:2px size:20px r:full bg:bg-raised flex align-items:center justify-content:center transition:left|var(--duration-fast) ${checked ? 'left:22px' : 'left:2px'}`}
	>
		{#if checked}
			<Check size={12} class="fg:accent" aria-hidden="true" />
		{/if}
	</span>
</button>
