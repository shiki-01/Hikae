<script lang="ts">
	import { Check, Eye, Circle } from '@lucide/svelte';
	import { t } from '#lib/i18n/index.js';
	import type { Choice, ConflictFile } from '#lib/api/types.js';
	import Button from './Button.svelte';
	import Checkbox from './Checkbox.svelte';
	import Radio from './Radio.svelte';
	import { alternateName, conflictDetail, conflictOptions } from './conflict-item';

	interface Props {
		conflict: ConflictFile;
		choice: Choice | null;
		keepBoth: boolean;
		onchoice: (choice: Choice) => void;
		onkeepboth: (keep: boolean) => void;
		oncompare?: () => void;
		class?: string;
	}

	let {
		conflict,
		choice,
		keepBoth,
		onchoice,
		onkeepboth,
		oncompare,
		class: className = ''
	}: Props = $props();

	const options = $derived(conflictOptions(conflict.kind));
	const unused = $derived<Choice | null>(
		choice === null ? null : choice === 'this' ? 'cloud' : 'this'
	);

	const keepBothHint = $derived(
		unused
			? t('conflict.keep_both_hint', {
					name: alternateName(
						conflict.path,
						unused,
						unused === 'cloud' ? conflict.cloudSavedAt : conflict.thisPcSavedAt
					)
				})
			: undefined
	);
</script>

<section
	aria-labelledby={`${conflict.path}-title`}
	class={`flex flex-direction:column gap:3 p:4 r:md bg:bg-raised ${
		choice ? 'b:1px|solid|accent' : 'b:1px|solid|state-danger'
	} ${className}`}
>
	<div class="flex align-items:center justify-content:space-between gap:3">
		<h3
			id={`${conflict.path}-title`}
			class="m:0 type-heading overflow:hidden text-overflow:ellipsis"
		>
			{conflict.path}
		</h3>
		<span class="inline-flex align-items:center gap:1 type-small fg:fg-muted flex-shrink:0">
			{#if choice}
				<Check size={14} class="fg:state-saved" aria-hidden="true" />
				{t('conflict.chosen')}
			{:else}
				<Circle size={14} class="fg:state-danger" aria-hidden="true" />
				{t('conflict.unchosen')}
			{/if}
		</span>
	</div>

	<div class="flex flex-direction:column gap:2" role="radiogroup" aria-label={conflict.path}>
		{#each options as option (option.choice)}
			<Radio
				name={`choice-${conflict.path}`}
				value={option.choice}
				group={choice}
				label={t(option.label)}
				description={conflictDetail(option.choice, conflict)}
				onselect={(value) => onchoice(value as Choice)}
			/>
		{/each}
	</div>

	{#if conflict.kind === 'both'}
		<div
			class="flex align-items:start justify-content:space-between gap:3 pt:3 bt:1px|solid|border"
		>
			<Checkbox
				checked={keepBoth}
				label={t('conflict.keep_both')}
				description={keepBothHint}
				onchange={onkeepboth}
			/>
			{#if oncompare}
				<Button size="sm" variant="secondary" onclick={oncompare} class="flex-shrink:0">
					<Eye size={14} aria-hidden="true" />
					{t('file_action.compare')}
				</Button>
			{/if}
		</div>
	{/if}
</section>
