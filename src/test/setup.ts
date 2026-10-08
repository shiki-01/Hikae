// jsdom に無い API を、スモークテストが動く最小限で補う（ブラウザ環境のときだけ）
if (typeof HTMLDialogElement !== 'undefined') {
	const proto = HTMLDialogElement.prototype;
	proto.showModal ??= function (this: HTMLDialogElement) {
		this.setAttribute('open', '');
	};
	proto.show ??= function (this: HTMLDialogElement) {
		this.setAttribute('open', '');
	};
	proto.close ??= function (this: HTMLDialogElement) {
		this.removeAttribute('open');
	};
}

if (typeof window !== 'undefined') {
	window.matchMedia ??= ((query: string) => ({
		matches: false,
		media: query,
		onchange: null,
		addEventListener: () => {},
		removeEventListener: () => {},
		addListener: () => {},
		removeListener: () => {},
		dispatchEvent: () => false
	})) as typeof window.matchMedia;

	class NoopObserver {
		observe() {}
		unobserve() {}
		disconnect() {}
	}
	globalThis.ResizeObserver ??= NoopObserver as unknown as typeof ResizeObserver;
	Element.prototype.scrollIntoView ??= () => {};
}
