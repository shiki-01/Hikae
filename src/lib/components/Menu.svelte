<script lang="ts" module>
	import type { Component } from 'svelte';

	export type MenuEntry =
		| { type?: 'item'; label: string; icon?: Component; disabled?: boolean; onselect: () => void }
		| { type: 'separator' };
</script>

<script lang="ts">
	import { nextIndex } from '#lib/utils/keyboard.js';

	interface Props {
		items: MenuEntry[];
		open: boolean;
		label: string;
		align?: 'start' | 'end';
		ignore?: HTMLElement;
		onclose: (restoreFocus: boolean) => void;
		class?: string;
	}

	let {
		items,
		open,
		label,
		align = 'start',
		ignore,
		onclose,
		class: className = ''
	}: Props = $props();

	let list = $state<HTMLUListElement>();

	function buttons(): HTMLButtonElement[] {
		return list
			? Array.from(list.querySelectorAll<HTMLButtonElement>('button:not(:disabled)'))
			: [];
	}

	$effect(() => {
		if (open) buttons()[0]?.focus();
	});

	function onKeydown(event: KeyboardEvent) {
		if (event.key === 'Escape') {
			event.preventDefault();
			event.stopPropagation();
			onclose(true);
			return;
		}
		if (event.key === 'Tab') {
			onclose(false);
			return;
		}
		const all = buttons();
		const current = all.indexOf(document.activeElement as HTMLButtonElement);
		const target = nextIndex(event.key, current < 0 ? 0 : current, all.length, 'vertical');
		if (target !== null) {
			event.preventDefault();
			all[target]?.focus();
		}
	}

	function onWindowPointerDown(event: PointerEvent) {
		if (!open) return;
		const target = event.target as Node;
		if (list?.contains(target) || ignore?.contains(target)) return;
		onclose(false);
	}
</script>

<svelte:window onpointerdown={onWindowPointerDown} />

{#if open}
	<ul
		bind:this={list}
		role="menu"
		aria-label={label}
		tabindex="-1"
		onkeydown={onKeydown}
		class={`position:absolute top:100% mt:1 z:50 m:0 p:1 list-style:none min-w:200px bg:bg-raised b:1px|solid|border r:md shadow:overlay ${align === 'end' ? 'right:0' : 'left:0'} ${className}`}
	>
		{#each items as entry, index (index)}
			{#if entry.type === 'separator'}
				<li role="separator" class="h:1px bg:border my:1"></li>
			{:else}
				{@const Icon = entry.icon}
				<li role="none">
					<button
						type="button"
						role="menuitem"
						tabindex="-1"
						disabled={entry.disabled}
						onclick={() => {
							onclose(true);
							entry.onselect();
						}}
						class="flex align-items:center gap:2 w:100% px:3 py:2 r:sm b:0 bg:transparent fg:fg type-body text-align:left cursor:pointer bg:bg-subtle:hover bg:bg-subtle:focus opacity:.5:disabled cursor:not-allowed:disabled"
					>
						{#if Icon}
							<Icon size={16} class="fg:fg-muted" aria-hidden="true" />
						{/if}
						{entry.label}
					</button>
				</li>
			{/if}
		{/each}
	</ul>
{/if}
