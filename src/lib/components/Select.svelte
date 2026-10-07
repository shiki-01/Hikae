<script lang="ts" module>
	export interface SelectOption {
		value: string;
		label: string;
		description?: string;
		disabled?: boolean;
	}
</script>

<script lang="ts">
	import { Check, ChevronDown } from '@lucide/svelte';
	import { nextIndex } from '#lib/utils/keyboard.js';

	interface Props {
		options: SelectOption[];
		value: string;
		ariaLabel: string;
		variant?: 'standard' | 'timepoint';
		disabled?: boolean;
		placeholder?: string;
		onchange?: (value: string) => void;
		class?: string;
	}

	let {
		options,
		value = $bindable(),
		ariaLabel,
		variant = 'standard',
		disabled = false,
		placeholder = '',
		onchange,
		class: className = ''
	}: Props = $props();

	const id = $props.id();
	let open = $state(false);
	let active = $state(0);
	let host = $state<HTMLDivElement>();
	let trigger = $state<HTMLButtonElement>();
	let list = $state<HTMLUListElement>();

	const selected = $derived(options.find((o) => o.value === value));

	function openList() {
		if (disabled) return;
		const index = options.findIndex((o) => o.value === value);
		active = index < 0 ? 0 : index;
		open = true;
	}

	function closeList(restoreFocus: boolean) {
		open = false;
		if (restoreFocus) trigger?.focus();
	}

	function choose(option: SelectOption) {
		if (option.disabled) return;
		value = option.value;
		onchange?.(option.value);
		closeList(true);
	}

	$effect(() => {
		if (open) list?.focus();
	});

	function onTriggerKeydown(event: KeyboardEvent) {
		if (['ArrowDown', 'ArrowUp', 'Enter', ' '].includes(event.key)) {
			event.preventDefault();
			openList();
		}
	}

	function onListKeydown(event: KeyboardEvent) {
		if (event.key === 'Escape') {
			event.preventDefault();
			event.stopPropagation();
			closeList(true);
			return;
		}
		if (event.key === 'Tab') {
			closeList(false);
			return;
		}
		if (event.key === 'Enter' || event.key === ' ') {
			event.preventDefault();
			const option = options[active];
			if (option) choose(option);
			return;
		}
		const target = nextIndex(event.key, active, options.length, 'vertical');
		if (target !== null) {
			event.preventDefault();
			active = target;
			document.getElementById(`${id}-opt-${target}`)?.scrollIntoView({ block: 'nearest' });
		}
	}

	function onListClick(event: MouseEvent) {
		const item = (event.target as HTMLElement).closest<HTMLElement>('[role="option"]');
		const option = item ? options[Number(item.dataset.index)] : undefined;
		if (option) choose(option);
	}

	function onWindowPointerDown(event: PointerEvent) {
		if (open && !host?.contains(event.target as Node)) closeList(false);
	}
</script>

<svelte:window onpointerdown={onWindowPointerDown} />

<div bind:this={host} class={`position:relative ${className}`}>
	<button
		bind:this={trigger}
		type="button"
		{disabled}
		aria-label={ariaLabel}
		aria-haspopup="listbox"
		aria-expanded={open}
		aria-controls={`${id}-list`}
		onclick={() => (open ? closeList(false) : openList())}
		onkeydown={onTriggerKeydown}
		class="flex align-items:center justify-content:space-between gap:2 w:100% px:3 py:2 r:md bg:bg fg:fg b:1px|solid|border-strong text-align:left cursor:pointer opacity:.5:disabled cursor:not-allowed:disabled"
	>
		<span class="flex flex-direction:column min-w:0">
			<span class="type-body overflow:hidden text-overflow:ellipsis white-space:nowrap">
				{selected?.label ?? placeholder}
			</span>
			{#if variant === 'timepoint' && selected?.description}
				<span
					class="type-small fg:fg-muted overflow:hidden text-overflow:ellipsis white-space:nowrap"
				>
					{selected.description}
				</span>
			{/if}
		</span>
		<ChevronDown size={16} class="fg:fg-muted flex-shrink:0" aria-hidden="true" />
	</button>

	{#if open}
		<ul
			bind:this={list}
			id={`${id}-list`}
			role="listbox"
			tabindex="-1"
			aria-label={ariaLabel}
			aria-activedescendant={`${id}-opt-${active}`}
			onkeydown={onListKeydown}
			onclick={onListClick}
			class="position:absolute top:100% left:0 mt:1 z:50 m:0 p:1 list-style:none w:100% min-w:240px max-h:320px overflow-y:auto bg:bg-raised b:1px|solid|border r:md shadow:overlay"
		>
			{#each options as option, index (option.value)}
				<li
					id={`${id}-opt-${index}`}
					role="option"
					aria-selected={option.value === value}
					aria-disabled={option.disabled || undefined}
					data-index={index}
					class={`flex align-items:center justify-content:space-between gap:2 px:3 py:2 r:sm cursor:pointer ${
						index === active ? 'bg:bg-subtle' : ''
					} ${option.disabled ? 'opacity:.5' : ''}`}
				>
					<span class="flex flex-direction:column min-w:0">
						<span class="type-body">{option.label}</span>
						{#if variant === 'timepoint' && option.description}
							<span class="type-small fg:fg-muted">{option.description}</span>
						{/if}
					</span>
					{#if option.value === value}
						<Check size={16} class="fg:accent flex-shrink:0" aria-hidden="true" />
					{/if}
				</li>
			{/each}
		</ul>
	{/if}
</div>
