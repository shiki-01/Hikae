<script lang="ts">
	import type { Snippet } from 'svelte';
	import { nextIndex } from '#lib/utils/keyboard.js';

	interface Tab {
		id: string;
		label: string;
	}

	interface Props {
		tabs: Tab[];
		selected: string;
		ariaLabel: string;
		panel: Snippet<[string]>;
		class?: string;
	}

	let { tabs, selected = $bindable(), ariaLabel, panel, class: className = '' }: Props = $props();

	const base = $props.id();
	let buttons = $state<HTMLButtonElement[]>([]);

	function onKeydown(event: KeyboardEvent, index: number) {
		const target = nextIndex(event.key, index, tabs.length, 'vertical');
		if (target === null) return;
		event.preventDefault();
		selected = tabs[target].id;
		buttons[target]?.focus();
	}
</script>

<div class={`flex flex:1 min-h:0 ${className}`}>
	<div
		role="tablist"
		aria-orientation="vertical"
		aria-label={ariaLabel}
		class="w:200px flex-shrink:0 flex flex-direction:column gap:1 p:2 bg:bg-subtle br:1px|solid|border overflow-y:auto"
	>
		{#each tabs as tab, index (tab.id)}
			{@const active = selected === tab.id}
			<button
				bind:this={buttons[index]}
				type="button"
				role="tab"
				id={`${base}-tab-${tab.id}`}
				aria-selected={active}
				aria-controls={`${base}-panel`}
				tabindex={active ? 0 : -1}
				onclick={() => (selected = tab.id)}
				onkeydown={(event) => onKeydown(event, index)}
				class={`px:3 py:2 r:md b:0 type-body text-align:left cursor:pointer ${
					active
						? 'bg:accent-subtle fg:fg font-weight:700'
						: 'bg:transparent fg:fg-muted fg:fg:hover bg:bg-subtle:hover'
				}`}
			>
				{tab.label}
			</button>
		{/each}
	</div>
	<div
		role="tabpanel"
		id={`${base}-panel`}
		aria-labelledby={`${base}-tab-${selected}`}
		tabindex="0"
		class="flex:1 min-w:0 overflow:auto p:6"
	>
		{@render panel(selected)}
	</div>
</div>
