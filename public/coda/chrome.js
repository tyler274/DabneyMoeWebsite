// The nav on every page: it floats once the page scrolls past 8px.
const nav = document.querySelector('nav.nav');
const onScroll = () => nav?.classList.toggle('scrolled', window.scrollY > 8);
window.addEventListener('scroll', onScroll, { passive: true });
onScroll();
