<script lang="ts">
	import { t } from '#lib/i18n/index.js';
	import type { IgnoreKind } from '#lib/api/types.js';
	import Button from '#lib/components/Button.svelte';
	import Dialog from '#lib/components/Dialog.svelte';
	import SegmentedControl from '#lib/components/SegmentedControl.svelte';
	import TextField from '#lib/components/TextField.svelte';
	import { IGNORE_KIND_OPTIONS, canSubmitIgnoreInput, normalizeIgnoreInput } from './ignore-model';

	interface Props {
		open: boolean;
		/** 追加の処理中は true */
		busy: boolean;
		onsubmit: (kind: IgnoreKind, value: string) => void;
		oncancel: () => void;
	}

	let { open, busy, onsubmit, oncancel }: Props = $props();

	let kind = $state<IgnoreKind>('extension');
	let input = $state('');

	const option = $derived(IGNORE_KIND_OPTIONS.find((o) => o.value === kind));
	const submittable = $derived(canSubmitIgnoreInput(kind, input));

	// 開くたびに、種類と入力を初期値に戻す
	$effect(() => {
		if (open) {
			kind = 'extension';
			input = '';
		}
	});

	function submit() {
		if (!submittable || busy) return;
		onsubmit(kind, normalizeIgnoreInput(kind, input));
	}
</script>

<Dialog {open} variant="form" title={t('ignore.add_title')} {busy} onclose={oncancel}>
	<div class="flex flex-direction:column gap:4">
		<div class="flex flex-direction:column gap:2">
			<span class="type-small fg:fg-muted">{t('ignore.kind_label')}</span>
			<SegmentedControl
				ariaLabel={t('ignore.kind_label')}
				value={kind}
				options={IGNORE_KIND_OPTIONS.map((o) => ({ value: o.value, label: t(o.label) }))}
				onchange={(next) => (kind = next as IgnoreKind)}
			/>
		</div>

		{#if option}
			<TextField
				label={t(option.inputLabel)}
				placeholder={t(option.placeholder)}
				description={t(option.hint)}
				bind:value={input}
				onkeydown={(event) => {
					if (event.key === 'Enter') submit();
				}}
			/>
		{/if}
	</div>

	{#snippet actions()}
		<Button variant="ghost" disabled={busy} onclick={oncancel}>{t('ignore.cancel')}</Button>
		<Button loading={busy} disabled={!submittable || busy} onclick={submit}>
			{t('ignore.submit')}
		</Button>
	{/snippet}
</Dialog>
