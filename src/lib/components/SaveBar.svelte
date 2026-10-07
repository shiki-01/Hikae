<script lang="ts">
	import { Sparkles } from '@lucide/svelte';
	import { t } from '#lib/i18n/index.js';
	import Button from './Button.svelte';
	import Spinner from './Spinner.svelte';
	import TextField from './TextField.svelte';

	interface Props {
		memo: string;
		hasChanges: boolean;
		saving?: boolean;
		aiState?: 'idle' | 'generating' | 'ready';
		onsave: () => void;
		onsuggest?: () => void;
		class?: string;
	}

	let {
		memo = $bindable(),
		hasChanges,
		saving = false,
		aiState,
		onsave,
		onsuggest,
		class: className = ''
	}: Props = $props();
</script>

<section
	aria-label={t('save_bar.label')}
	class={`flex align-items:end gap:4 px:4 py:3 flex-shrink:0 bg:bg-subtle bt:1px|solid|border ${className}`}
>
	<TextField
		bind:value={memo}
		multiline
		rows={1}
		maxRows={2}
		label={t('save_bar.memo')}
		placeholder={hasChanges ? t('save_bar.placeholder') : t('save_bar.empty')}
		disabled={!hasChanges || saving}
		class="flex:1"
	/>
	<div class="flex align-items:center gap:3 flex-shrink:0">
		{#if aiState}
			<Button
				variant="ghost"
				size="sm"
				disabled={!hasChanges || saving || aiState === 'generating'}
				onclick={onsuggest}
			>
				{#if aiState === 'generating'}
					<Spinner size="sm" label={t('save_bar.ai_generating')} />
					{t('save_bar.ai_generating')}
				{:else}
					<Sparkles size={14} aria-hidden="true" />
					{t('save_bar.ai_suggestion')}
				{/if}
			</Button>
		{/if}
		<Button disabled={!hasChanges} loading={saving} onclick={onsave}>
			{saving ? t('save_bar.saving') : t('save_bar.button')}
		</Button>
	</div>
</section>
