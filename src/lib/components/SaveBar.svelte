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
		/** AI が作ったメモの提案（無ければ null）。押したときだけメモ欄に入る */
		suggestion?: string | null;
		/** AI の提案を取得中 */
		suggesting?: boolean;
		onsave: () => void;
		onusesuggestion?: (text: string) => void;
		class?: string;
	}

	let {
		memo = $bindable(),
		hasChanges,
		saving = false,
		suggestion = null,
		suggesting = false,
		onsave,
		onusesuggestion,
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
		{#if suggesting}
			<span class="flex align-items:center gap:2 type-small fg:fg-muted">
				<Spinner size="sm" label={t('save_bar.ai_generating')} />
				{t('save_bar.ai_generating')}
			</span>
		{:else if suggestion && hasChanges}
			<Button
				variant="ghost"
				size="sm"
				disabled={saving}
				title={suggestion}
				ariaLabel={t('save_bar.ai_use')}
				onclick={() => onusesuggestion?.(suggestion)}
			>
				<Sparkles size={14} aria-hidden="true" />
				<span class="max-w:240px overflow:hidden text-overflow:ellipsis white-space:nowrap">
					{suggestion}
				</span>
			</Button>
		{/if}
		<Button disabled={!hasChanges} loading={saving} onclick={onsave}>
			{saving ? t('save_bar.saving') : t('save_bar.button')}
		</Button>
	</div>
</section>
