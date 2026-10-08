<script lang="ts">
	import { ShieldCheck } from '@lucide/svelte';
	import { t } from '#lib/i18n/index.js';
	import Button from '#lib/components/Button.svelte';
	import Dialog from '#lib/components/Dialog.svelte';
	import { interruptedOperationName } from './interrupted';

	interface Props {
		open: boolean;
		/** 途中で止まった操作の名前。不明なら null */
		operation: string | null;
		onclose: () => void;
	}

	let { open, operation, onclose }: Props = $props();
</script>

<Dialog
	{open}
	variant="warning"
	title={t('interrupted.title')}
	description={t('interrupted.description', { name: interruptedOperationName(operation) })}
	{onclose}
>
	<p class="m:0 type-body">{t('interrupted.next')}</p>

	{#snippet footerNote()}
		<div class="flex align-items:start gap:2 p:3 r:md bg:bg-subtle">
			<ShieldCheck size={18} class="fg:state-saved flex-shrink:0 mt:2px" aria-hidden="true" />
			<p class="m:0 type-body">{t('interrupted.safe')}</p>
		</div>
	{/snippet}

	{#snippet actions()}
		<Button onclick={onclose}>{t('interrupted.close')}</Button>
	{/snippet}
</Dialog>
