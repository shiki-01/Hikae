<script lang="ts">
	import { Sparkles } from '@lucide/svelte';
	import { t } from '#lib/i18n/index.js';
	import { suggestFailureKey, type SuggestState } from '#lib/features/ai/suggest.js';
	import Button from './Button.svelte';
	import Spinner from './Spinner.svelte';
	import TextField from './TextField.svelte';

	interface Props {
		memo: string;
		hasChanges: boolean;
		saving?: boolean;
		/** AI の提案を作れるか（機能が使え、変更がある）。false のときは提案の欄を出さない */
		canSuggest?: boolean;
		/** AI の提案の画面の状態。押したときだけ生成し、結果は「使う」でメモ欄に入る */
		suggestState?: SuggestState;
		onsave: () => void;
		onrequest?: () => void;
		onregenerate?: () => void;
		oncancel?: () => void;
		onusesuggestion?: (text: string) => void;
		class?: string;
	}

	let {
		memo = $bindable(),
		hasChanges,
		saving = false,
		canSuggest = false,
		suggestState = { kind: 'idle' },
		onsave,
		onrequest,
		onregenerate,
		oncancel,
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
		{#if canSuggest && hasChanges}
			<div class="flex align-items:center gap:2" aria-live="polite">
				{#if suggestState.kind === 'idle'}
					<Button variant="ghost" size="sm" onclick={() => onrequest?.()}>
						<Sparkles size={14} aria-hidden="true" />
						{t('save_bar.ai_request')}
					</Button>
				{:else if suggestState.kind === 'generating'}
					<span class="flex align-items:center gap:2 type-small fg:fg-muted">
						<Spinner size="sm" label={t('save_bar.ai_generating')} />
						{t('save_bar.ai_generating')}
					</span>
					<Button variant="ghost" size="sm" onclick={() => oncancel?.()}>
						{t('save_bar.ai_cancel')}
					</Button>
				{:else if suggestState.kind === 'result'}
					<span
						class="max-w:240px overflow:hidden text-overflow:ellipsis white-space:nowrap type-small"
						title={suggestState.text}
					>
						{suggestState.text}
					</span>
					<Button
						variant="ghost"
						size="sm"
						disabled={saving}
						ariaLabel={t('save_bar.ai_use')}
						onclick={() =>
							onusesuggestion?.(suggestState.kind === 'result' ? suggestState.text : '')}
					>
						{t('save_bar.ai_use_button')}
					</Button>
					<Button variant="ghost" size="sm" onclick={() => onregenerate?.()}>
						{t('save_bar.ai_regenerate')}
					</Button>
				{:else}
					<span class="max-w:320px type-small fg:fg-muted">
						{t(suggestFailureKey(suggestState.failure))}
					</span>
					<Button variant="ghost" size="sm" onclick={() => onrequest?.()}>
						{t('save_bar.ai_retry')}
					</Button>
				{/if}
			</div>
		{/if}
		<Button disabled={!hasChanges} loading={saving} onclick={onsave}>
			{saving ? t('save_bar.saving') : t('save_bar.button')}
		</Button>
	</div>
</section>
