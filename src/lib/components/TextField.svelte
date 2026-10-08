<script lang="ts">
	import { AlertCircle } from '@lucide/svelte';
	import { t } from '#lib/i18n/index.js';

	interface Props {
		value?: string;
		label?: string;
		ariaLabel?: string;
		description?: string;
		error?: string;
		placeholder?: string;
		type?: 'text' | 'search' | 'url';
		multiline?: boolean;
		rows?: number;
		maxRows?: number;
		maxLength?: number;
		disabled?: boolean;
		readonly?: boolean;
		required?: boolean;
		oninput?: (value: string) => void;
		onblur?: () => void;
		onkeydown?: (event: KeyboardEvent) => void;
		inputmode?: 'text' | 'numeric';
		class?: string;
	}

	let {
		value = $bindable(''),
		label,
		ariaLabel,
		description,
		error,
		placeholder,
		type = 'text',
		multiline = false,
		rows = 1,
		maxRows = 4,
		maxLength,
		disabled = false,
		readonly = false,
		required = false,
		oninput,
		onblur,
		onkeydown,
		inputmode,
		class: className = ''
	}: Props = $props();

	const id = $props.id();
	const describedBy = $derived(
		[description ? `${id}-desc` : null, error ? `${id}-error` : null].filter(Boolean).join(' ') ||
			undefined
	);
	let area = $state<HTMLTextAreaElement>();

	$effect(() => {
		void value;
		const el = area;
		if (!el) return;
		el.style.height = 'auto';
		const style = getComputedStyle(el);
		const lineHeight = parseFloat(style.lineHeight) || 22;
		const chrome = parseFloat(style.paddingTop) + parseFloat(style.paddingBottom) + 2;
		const max = lineHeight * maxRows + chrome;
		el.style.height = `${Math.min(el.scrollHeight + 2, max)}px`;
		el.style.overflowY = el.scrollHeight + 2 > max ? 'auto' : 'hidden';
	});

	const frame = $derived(
		error ? 'b:1px|solid|state-danger' : 'b:1px|solid|border-strong b:1px|solid|accent:focus'
	);
	const control = $derived(
		`w:100% px:3 py:2 r:md bg:bg fg:fg type-body ${frame} opacity:.5:disabled cursor:not-allowed:disabled`
	);

	function handleInput(event: Event) {
		const target = event.currentTarget as HTMLInputElement | HTMLTextAreaElement;
		value = target.value;
		oninput?.(value);
	}
</script>

<div class={`flex flex-direction:column gap:1 ${className}`}>
	{#if label}
		<label for={id} class="type-small font-weight:500 fg:fg">{label}</label>
	{/if}
	{#if description}
		<p id={`${id}-desc`} class="m:0 type-small fg:fg-muted">{description}</p>
	{/if}
	{#if multiline}
		<textarea
			bind:this={area}
			{id}
			{value}
			{placeholder}
			{disabled}
			{readonly}
			{required}
			{rows}
			maxlength={maxLength}
			aria-label={label ? undefined : ariaLabel}
			aria-invalid={error ? true : undefined}
			aria-describedby={describedBy}
			oninput={handleInput}
			class={`${control} resize:none`}></textarea>
	{:else}
		<input
			{id}
			{type}
			{value}
			{placeholder}
			{disabled}
			{readonly}
			{required}
			maxlength={maxLength}
			aria-label={label ? undefined : ariaLabel}
			aria-invalid={error ? true : undefined}
			aria-describedby={describedBy}
			oninput={handleInput}
			{onblur}
			{onkeydown}
			{inputmode}
			class={control}
		/>
	{/if}
	{#if error || maxLength}
		<div class="flex justify-content:space-between gap:2">
			{#if error}
				<p id={`${id}-error`} class="m:0 flex align-items:center gap:1 type-small fg:state-danger">
					<AlertCircle size={14} aria-hidden="true" />
					{error}
				</p>
			{:else}
				<span></span>
			{/if}
			{#if maxLength}
				<span class="type-small fg:fg-muted"
					>{t('field.count', { n: value.length, max: maxLength })}</span
				>
			{/if}
		</div>
	{/if}
</div>
