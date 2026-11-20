/* The Duet story (src/pages/phones.rs, styled by /css/duet.css). Site-owned; it shares
   nothing with /homepage/site.js except the stage scaling and art site.js applies to
   every .stage-wrap and .frame. The markup is the story's last frame, and reduced
   motion keeps it. Otherwise each pass, while the stage is on screen, starts from the
   opening frame. Duet does the noticing and the following up; Maya says yes: Today
   shows what Duet noticed, Duet offers a morning Check-in and she accepts it; on
   Monday the Check-in returns by itself and Duet prepares an update to her page for
   her doctor; she reviews it, saves it, previews it, and the story ends on the page
   her clinician receives. */
(function(){
  'use strict';
  var stage = document.querySelector('.duet-story');
  if(!stage || matchMedia('(prefers-reduced-motion: reduce)').matches) return;
  var $ = function(s){ return stage.querySelector(s); };
  var $$ = function(s){ return [].slice.call(stage.querySelectorAll(s)); };
  var wrap = stage.parentNode;

  var insight = $('.ds-insight'), coming = $('.ds-coming');
  var chat = $('.ds-chat'), thread = $('.ds-thread'), typing = $('.ds-typing');
  var input = $('.ds-input'), send = $('.ds-send'), placeholder = input.textContent;
  var accept = $('.ds-accept'), reviewLink = $('.ds-review-link');
  var update = $('.ds-update'), review = $('.ds-review'), save = $('.ds-save');
  var toPreview = $('.ds-to-preview'), open = $('.ds-open'), clinician = $('.ds-clin');
  // The thread, in order: Duet's offer, the Check-in's receipt, Monday, the
  // Check-in's return, "Yes, add them", and the update Duet prepared.
  var msgs = $$('.ds-msg');
  var offer = msgs[0], receipt = msgs[1], monday = msgs[2], back = msgs[3], yes = msgs[4], prepared = msgs[5];

  // Every class the story sets goes through mark(), so the opening frame clears
  // exactly those and nothing else on the page.
  var marked = [];
  function mark(el, name){ el.classList.add(name); marked.push([el, name]); }
  function opening(){
    marked.forEach(function(m){ m[0].classList.remove(m[1]); }); marked = [];
    offer.classList.remove('ds-accepted');
    input.textContent = placeholder;
    thread.style.transform = '';
    stage.dataset.day = 'thu';
    stage.dataset.step = 'rest';
    stage.dataset.focus = '1';
  }
  function reset(){ stage.classList.add('notrans'); opening(); stage.getBoundingClientRect(); stage.classList.remove('notrans'); }
  stage.classList.add('ds-play');
  reset();

  // A pass's pauses end it once it is no longer the current pass (the stage left the
  // screen); STOP unwinds the story from wherever it was.
  var STOP = {}, token = 0;
  function pauses(my){
    return function(ms){ return new Promise(function(ok, stop){ setTimeout(function(){ my === token ? ok() : stop(STOP); }, ms); }); };
  }
  async function press(el, w){ mark(el, 'press'); await w(260); el.classList.remove('press'); }
  // On narrow stages the phone doing the work glides to the front first.
  async function focus(n, w){ stage.dataset.focus = n; if(wrap.classList.contains('compact')) await w(1000); }
  // Something Duet brought arrives with a ring that fades once it has been seen.
  async function arrive(el, w){ mark(el, 'glow'); await w(1600); el.classList.remove('glow'); }
  // The conversation keeps its newest message in view, as a chat scrolls.
  function follow(){
    var over = thread.offsetHeight - chat.clientHeight;
    thread.style.transform = over > 0 ? 'translateY(' + -over + 'px)' : '';
  }
  // The companion works where its message will appear, then the message arrives.
  async function message(msg, ms, w){
    thread.insertBefore(typing, msg); mark(typing, 'on'); follow(); await w(ms);
    typing.classList.remove('on'); mark(msg, 'on'); follow();
  }
  // Maya's reply: typed into the composer and sent.
  async function say(words, msg, w){
    mark(input, 'typed');
    for(var i = 1; i <= words.length; i++){ input.textContent = words.slice(0, i); await w(45); }
    await w(300); await press(send, w);
    input.classList.remove('typed'); input.textContent = placeholder;
    mark(msg, 'on'); follow();
  }

  async function story(w){
    await w(900);
    // 1 · Duet notices: the weekly insight appears in Today on its own.
    mark(insight, 'on'); await arrive(insight, w);
    // 2 · Duet follows up: it offers a Check-in, and she accepts it where it is offered.
    await focus('2', w);
    await message(offer, 1300, w); await w(3200);
    await press(accept, w); mark(offer, 'ds-accepted'); follow(); await w(500);
    await message(receipt, 700, w); mark(coming, 'on'); await arrive(receipt, w); await w(400);
    // Monday: the Check-in comes back by itself, and Duet offers to update her page.
    stage.dataset.day = 'mon'; mark(monday, 'on'); follow(); await w(900);
    await message(back, 1300, w); await w(2800);
    await say('Yes, add them', yes, w); await w(400);
    await message(prepared, 1100, w); stage.dataset.step = 'ready';
    await arrive(reviewLink, w);
    await press(reviewLink, w);
    // 3 · For my doctor: the update waits for her; she reviews, saves and previews it.
    await focus('3', w);
    await arrive(update, w);
    await press(review, w); stage.dataset.step = 'review'; await w(3400);
    await press(save, w); stage.dataset.step = 'saved'; await w(2400);
    await press(toPreview, w); stage.dataset.step = 'preview'; await w(2600);
    // The ending: the page her clinician receives.
    await press(open, w); mark(clinician, 'on'); await w(6500);
  }
  // Between passes: the clinician page leaves, each screen is covered while the
  // phones go back, and the opening frame is set unseen.
  async function cover(w){
    clinician.classList.remove('on'); await w(600);
    stage.classList.add('ds-out'); stage.dataset.focus = '1';
    await w(wrap.classList.contains('compact') ? 1000 : 700);
    reset(); stage.classList.remove('ds-out'); await w(600);
  }

  // Runs only while the stage is on screen. The first pass starts from the opening
  // frame set above; every later start (a loop, or a return) covers and resets first.
  var inView = false, running = false, played = false;
  function refresh(){
    if(inView && !running){
      running = true;
      var w = pauses(++token);
      (async function(){
        try { for(;;){ if(played) await cover(w); played = true; await story(w); } }
        catch(e){ if(e !== STOP) throw e; }
      })();
    } else if(!inView && running){ token++; running = false; }
  }
  // Starts once a third shows, or as soon as any shows when it is on the first screen.
  new IntersectionObserver(function(es){
    var e = es[0], firstScreen = stage.getBoundingClientRect().top + scrollY < innerHeight;
    if(e.intersectionRatio >= .35 || (firstScreen && e.isIntersecting)) inView = true;
    else if(!e.isIntersecting) inView = false;
    refresh();
  }, {threshold:[0, .35]}).observe(stage);
})();
