<script lang="ts">
	import { nextIndex } from '#lib/utils/keyboard.js';

	interface Option {
		value: string;
		label: string;
	}

	interface Props {
		options: Option[];
		value: string;
		ariaLabel: string;
		onchange?: (value: string) => void;
		class?: string;
	}

	let {
		options,
		value = $bindable(),
		ariaLabel,
		onchange,
		class: className = ''
	}: Props = $props();

	let buttons = $state<HTMLButtonElement[]>([]);

	function select(next: string) {
		value = next;
		onchange?.(next);
	}

	function onKeydown(event: KeyboardEvent, index: number) {
		const target = nextIndex(event.key, index, options.length, 'horizontal');
		if (target === null) return;
		event.preventDefault();
		select(options[target].value);
		buttons[target]?.focus();
	}
</script>

<div
	role="radiogroup"
	aria-label={ariaLabel}
	class={`inline-flex gap:1 p:1 r:md bg:border ${className}`}
>
	{#each options as option, index (option.value)}
		{@const selected = value === option.value}
		<button
			bind:this={buttons[index]}
			type="button"
			role="radio"
			aria-checked={selected}
			tabindex={selected ? 0 : -1}
			onclick={() => select(option.value)}
			onkeydown={(event) => onKeydown(event, index)}
			class={`px:3 py:1 r:sm b:0 type-small font-weight:500 cursor:pointer transition:background-color|var(--duration-fast) ${
				selected ? 'bg:bg-raised fg:fg shadow:sm' : 'bg:transparent fg:fg-muted fg:fg:hover'
			}`}
		>
			{option.label}
		</button>
	{/each}
</div>
