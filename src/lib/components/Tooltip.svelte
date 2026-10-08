<script lang="ts">
	import type { Snippet } from 'svelte';

	interface Props {
		text: string;
		position?: 'top' | 'bottom' | 'bottom-end' | 'left' | 'right';
		children: Snippet;
		class?: string;
	}

	let { text, position = 'bottom', children, class: className = '' }: Props = $props();

	const id = $props.id();
	let host = $state<HTMLSpanElement>();
	let visible = $state(false);

	$effect(() => {
		const el = host;
		if (!el) return;
		const show = () => queueMicrotask(() => (visible = true));
		const hide = () => queueMicrotask(() => (visible = false));
		const onKey = (event: KeyboardEvent) => {
			if (event.key === 'Escape') hide();
		};
		el.addEventListener('mouseenter', show);
		el.addEventListener('mouseleave', hide);
		el.addEventListener('focusin', show);
		el.addEventListener('focusout', hide);
		el.addEventListener('keydown', onKey);
		return () => {
			el.removeEventListener('mouseenter', show);
			el.removeEventListener('mouseleave', hide);
			el.removeEventListener('focusin', show);
			el.removeEventListener('focusout', hide);
			el.removeEventListener('keydown', onKey);
		};
	});

	const placement = {
		top: 'bottom:100% left:50% transform:translateX(-50%) mb:1',
		bottom: 'top:100% left:50% transform:translateX(-50%) mt:1',
		// 右端をそろえて下に出す（右端のボタンで、画面の外や隣の操作にはみ出さないため）
		'bottom-end': 'top:100% right:0 mt:1',
		left: 'right:100% top:50% transform:translateY(-50%) mr:1',
		right: 'left:100% top:50% transform:translateY(-50%) ml:1'
	};
</script>

<span
	bind:this={host}
	aria-describedby={visible ? id : undefined}
	class={`position:relative inline-flex ${className}`}
>
	{@render children()}
	{#if visible}
		<span
			{id}
			role="tooltip"
			class={`position:absolute z:50 px:2 py:1 r:sm bg:fg fg:bg type-small w:max-content max-w:280px pointer-events:none ${placement[position]}`}
		>
			{text}
		</span>
	{/if}
</span>
