<script lang="ts">
	import { Check, User, Users } from '@lucide/svelte';
	import { t } from '#lib/i18n/index.js';
	import type { Owner } from '#lib/api/types.js';
	import { nextIndex } from '#lib/utils/keyboard.js';

	interface Props {
		owners: Owner[];
		value: string;
		ariaLabel: string;
		onchange?: (id: string) => void;
		class?: string;
	}

	let { owners, value = $bindable(), ariaLabel, onchange, class: className = '' }: Props = $props();

	let buttons = $state<HTMLButtonElement[]>([]);

	function select(owner: Owner) {
		if (!owner.canCreate) return;
		value = owner.id;
		onchange?.(owner.id);
	}

	function onKeydown(event: KeyboardEvent, index: number) {
		const enabled = owners.map((o, i) => (o.canCreate ? i : -1)).filter((i) => i >= 0);
		const position = enabled.indexOf(index);
		const target = nextIndex(event.key, position, enabled.length, 'both');
		if (target === null) return;
		event.preventDefault();
		const next = enabled[target];
		select(owners[next]);
		buttons[next]?.focus();
	}
</script>

<div
	role="radiogroup"
	aria-label={ariaLabel}
	class={`flex flex-direction:column gap:2 ${className}`}
>
	{#each owners as owner, index (owner.id)}
		{@const selected = owner.id === value}
		<button
			bind:this={buttons[index]}
			type="button"
			role="radio"
			aria-checked={selected}
			aria-disabled={!owner.canCreate}
			aria-describedby={owner.canCreate ? undefined : `owner-${owner.id}-reason`}
			tabindex={selected ? 0 : -1}
			onclick={() => select(owner)}
			onkeydown={(event) => onKeydown(event, index)}
			class={`flex align-items:center gap:3 px:3 py:2 r:md text-align:left ${
				owner.canCreate ? 'cursor:pointer' : 'cursor:not-allowed opacity:.6'
			} ${
				selected ? 'bg:accent-subtle b:1px|solid|accent' : 'bg:bg-raised b:1px|solid|border-strong'
			} fg:fg`}
		>
			<span
				class="size:32px r:full bg:bg-subtle flex align-items:center justify-content:center flex-shrink:0"
				aria-hidden="true"
			>
				{#if owner.kind === 'org'}
					<Users size={16} class="fg:fg-muted" />
				{:else}
					<User size={16} class="fg:fg-muted" />
				{/if}
			</span>
			<span class="flex:1 flex flex-direction:column min-w:0">
				<span class="type-body">
					{owner.kind === 'org'
						? t('add_project.owner_org', { name: owner.name })
						: t('add_project.owner_personal_named', { name: owner.name })}
				</span>
				{#if !owner.canCreate}
					<span id={`owner-${owner.id}-reason`} class="type-small fg:fg-muted">
						{t('add_project.owner_no_permission')}
					</span>
				{/if}
			</span>
			{#if selected}
				<Check size={16} class="fg:accent flex-shrink:0" aria-hidden="true" />
			{/if}
		</button>
	{/each}
</div>
