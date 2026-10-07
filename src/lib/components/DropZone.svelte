<script lang="ts">
	import { AlertCircle, Upload } from '@lucide/svelte';
	import { t } from '#lib/i18n/index.js';
	import Button from './Button.svelte';

	interface Props {
		dragging?: boolean;
		tooLarge?: boolean;
		disabled?: boolean;
		onfiles?: (files: File[]) => void;
		class?: string;
	}

	let {
		dragging = false,
		tooLarge = false,
		disabled = false,
		onfiles,
		class: className = ''
	}: Props = $props();

	let zone = $state<HTMLDivElement>();
	let input = $state<HTMLInputElement>();
	let over = $state(false);

	$effect(() => {
		const el = zone;
		if (!el) return;
		const enter = (event: DragEvent) => {
			event.preventDefault();
			over = true;
		};
		const leave = () => (over = false);
		const drop = (event: DragEvent) => {
			event.preventDefault();
			event.stopPropagation();
			over = false;
			if (disabled || !event.dataTransfer?.files.length) return;
			onfiles?.(Array.from(event.dataTransfer.files));
		};
		el.addEventListener('dragenter', enter);
		el.addEventListener('dragover', enter);
		el.addEventListener('dragleave', leave);
		el.addEventListener('drop', drop);
		return () => {
			el.removeEventListener('dragenter', enter);
			el.removeEventListener('dragover', enter);
			el.removeEventListener('dragleave', leave);
			el.removeEventListener('drop', drop);
		};
	});

	function pick(event: Event) {
		const files = (event.currentTarget as HTMLInputElement).files;
		if (files?.length) onfiles?.(Array.from(files));
		if (input) input.value = '';
	}

	const mode = $derived(
		tooLarge ? 'large' : disabled ? 'disabled' : over || dragging ? 'over' : 'idle'
	);
	const looks = {
		idle: 'b:2px|dashed|border-strong bg:transparent',
		over: 'b:2px|dashed|accent bg:accent-subtle',
		large: 'b:2px|dashed|state-danger bg:transparent',
		disabled: 'b:2px|dashed|border opacity:.5'
	};
</script>

<div
	bind:this={zone}
	class={`flex flex-direction:column align-items:center gap:2 p:4 r:md text-align:center ${looks[mode]} ${className}`}
>
	{#if mode === 'large'}
		<AlertCircle size={24} class="fg:state-danger" aria-hidden="true" />
		<p class="m:0 type-body font-weight:500" role="alert">{t('dropzone.too_large')}</p>
	{:else if mode === 'over'}
		<Upload size={24} class="fg:accent" aria-hidden="true" />
		<p class="m:0 type-body font-weight:500">{t('dropzone.dragging')}</p>
	{:else}
		<Upload size={24} class="fg:fg-muted" aria-hidden="true" />
		<p class="m:0 type-small fg:fg-muted white-space:pre-line">{t('dropzone.text')}</p>
		<input bind:this={input} type="file" multiple class="hidden" tabindex="-1" onchange={pick} />
		<Button size="sm" variant="secondary" {disabled} onclick={() => input?.click()}>
			{t('dropzone.pick')}
		</Button>
	{/if}
</div>
