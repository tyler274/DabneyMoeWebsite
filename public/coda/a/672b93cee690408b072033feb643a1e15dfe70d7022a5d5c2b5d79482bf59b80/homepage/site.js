/* Coda Health homepage */
(function(){
  'use strict';
  var reduced = matchMedia('(prefers-reduced-motion: reduce)').matches;
  var $ = function(s, r){ return (r||document).querySelector(s); };
  var $$ = function(s, r){ return [].slice.call((r||document).querySelectorAll(s)); };
  var wait = function(ms){ return new Promise(function(r){ setTimeout(r, ms); }); };
  // Each section runs in its own function, so a name in one section cannot reach another.
  var section = function(sel, init){ var root = $(sel); if(root) init(root); };

  /* ================= painted landscapes =================
     Generated SVG paintings in the Coda palette: atmospheric
     ridges, mist, water and grain, rasterized once as images. */
  function rng(a){ return function(){ a|=0; a=a+0x6D2B79F5|0; var t=Math.imul(a^a>>>15,1|a); t=t+Math.imul(t^t>>>7,61|t)^t; return ((t^t>>>14)>>>0)/4294967296; }; }

  var PALETTES = {
    mist:{ sky:['#C9D3CC','#DEE3DC','#EEEDE4','#F3EFE5'], cloud:'#FFFFFF', shade:'#B9C4BD',
           ridges:['#B7C2BC','#9DADA4','#7E9387','#5E7768'], water:['#E3E6DE','#C7D0CA'], bank:'#3F5747', tree:['#34493C','#4A6352','#5C7562'], grain:.20 },
    sage:{ sky:['#C3CEC6','#D8DFD7','#E9EAE1','#F1EEE4'], cloud:'#FBFBF6', shade:'#AFBDB4',
           ridges:['#AEBBB3','#93A69A','#73897B','#56705F'], water:['#DCE2DA','#BFCAC2'], bank:'#3C5444', tree:['#31463A','#46604F','#59735F'], grain:.22 },
    dawn:{ sky:['#D6D6CB','#E8E3D5','#F2E9D9','#F5EEDF'], cloud:'#FFFDF6', shade:'#CFC9B9',
           ridges:['#C7C6B8','#A9B2A4','#889A8B','#647C6B'], water:['#EDE8DC','#D3D3C6'], bank:'#47604E', tree:['#3A5141','#506A56','#66806A'], grain:.20 },
    deep:{ sky:['#4D6457','#6F8476','#97A598','#B4BCAE'], cloud:'#C9CFC2', shade:'#5E7366',
           ridges:['#7E9184','#62786A','#4A6152','#33493B'], water:['#8E9E92','#5D7264'], bank:'#22332A', tree:['#1C2B22','#2A3D31','#36493C'], grain:.26 }
  };

  function ridgePath(r, W, H, base, amp, peaks, jag){
    var P = [];
    for(var i=0;i<peaks;i++) P.push({x:r()*W*1.2-W*.1, h:(.35+r()*.65)*amp, w:(.07+r()*.16)*W});
    var d = 'M0,'+H+' ';
    for(var x=0;x<=W;x+=8){
      var y = 0;
      for(var k=0;k<P.length;k++){ var p=P[k], u=(x-p.x)/p.w; y = Math.max(y, p.h*Math.exp(-u*u)); }
      y += (Math.sin(x*.031+peaks)*.5+Math.sin(x*.0123+amp)*.5)*jag + (r()-.5)*jag*.5;
      d += 'L'+x+','+(base-y).toFixed(1)+' ';
    }
    return d+'L'+W+','+H+' Z';
  }

  function paint(name, seed){
    // Soft-focus landscape: haze, distant ridges and still water, no hard edges.
    var p = PALETTES[name] || PALETTES.mist, r = rng(seed||1), W=1600, H=1000, horizon=600;
    var s = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 '+W+' '+H+'" preserveAspectRatio="xMidYMid slice">';
    s += '<defs>'
      + '<linearGradient id="sk" gradientUnits="userSpaceOnUse" x1="0" y1="0" x2="0" y2="'+H+'">'
      + '<stop offset="0" stop-color="'+p.sky[0]+'"/><stop offset=".42" stop-color="'+p.sky[1]+'"/><stop offset=".58" stop-color="'+p.sky[2]+'"/><stop offset=".64" stop-color="'+p.sky[3]+'"/>'
      + '<stop offset=".7" stop-color="'+p.water[0]+'"/><stop offset="1" stop-color="'+p.water[1]+'"/></linearGradient>'
      + '<filter id="f40" x="-20%" y="-20%" width="140%" height="140%"><feGaussianBlur stdDeviation="40"/></filter>'
      + '<filter id="f14" x="-10%" y="-10%" width="120%" height="120%"><feGaussianBlur stdDeviation="14"/></filter>'
      + '<filter id="f7" x="-10%" y="-10%" width="120%" height="120%"><feGaussianBlur stdDeviation="7"/></filter>'
      + '<filter id="f24" x="-10%" y="-10%" width="120%" height="120%"><feGaussianBlur stdDeviation="24"/></filter>'
      + '<filter id="grain"><feTurbulence type="fractalNoise" baseFrequency=".9" numOctaves="2" seed="'+(seed%31)+'" stitchTiles="stitch"/><feColorMatrix values="0 0 0 0 .08  0 0 0 0 .12  0 0 0 0 .1  0 0 0 1.2 -.5"/></filter>'
      + '</defs>';
    s += '<rect width="'+W+'" height="'+H+'" fill="url(#sk)"/>';
    // drifting cloud light
    s += '<g filter="url(#f40)">';
    for(var c=0;c<7;c++){
      s += '<ellipse cx="'+(r()*W).toFixed(0)+'" cy="'+(80+r()*300).toFixed(0)+'" rx="'+(220+r()*380).toFixed(0)+'" ry="'+(50+r()*80).toFixed(0)+'" fill="'+p.cloud+'" opacity="'+(.3+r()*.35).toFixed(2)+'"/>';
    }
    s += '</g>';
    // ridges: far ones dissolve into haze, the nearest stays soft
    var bases=[horizon-20, horizon+4, horizon+22], amps=[300,210,110], peaks=[5,6,7], blur=['f14','f7','f7'], op=[.55,.7,.85];
    for(var i=0;i<3;i++){
      s += '<path d="'+ridgePath(r,W,H,bases[i],amps[i],peaks[i],2)+'" fill="'+p.ridges[i]+'" opacity="'+op[i]+'" filter="url(#'+blur[i]+')"/>';
      s += '<rect x="-100" y="'+(bases[i]-50)+'" width="'+(W+200)+'" height="90" fill="'+p.sky[3]+'" opacity=".35" filter="url(#f24)"/>';
    }
    // still water with a faint mirrored ridge and mist on the shoreline
    s += '<rect x="-100" y="'+(horizon+20)+'" width="'+(W+200)+'" height="'+(H-horizon)+'" fill="url(#sk)" opacity=".9" filter="url(#f14)"/>';
    s += '<g opacity=".18" filter="url(#f14)" transform="translate(0,'+(2*(horizon+24))+') scale(1,-1)"><path d="'+ridgePath(rng(seed+7),W,H,horizon+24,150,6,2)+'" fill="'+p.ridges[2]+'"/></g>';
    s += '<rect x="-100" y="'+(horizon-6)+'" width="'+(W+200)+'" height="60" fill="'+p.sky[3]+'" opacity=".55" filter="url(#f14)"/>';
    // a soft shore in one corner gives depth without detail
    var left = r() > .5, sx = left ? 0 : W;
    s += '<ellipse cx="'+sx+'" cy="'+(H+40)+'" rx="'+(560+r()*200).toFixed(0)+'" ry="'+(230+r()*60).toFixed(0)+'" fill="'+p.ridges[3]+'" opacity=".55" filter="url(#f24)"/>';
    s += '<rect width="'+W+'" height="'+H+'" filter="url(#grain)" opacity="'+(p.grain*.7).toFixed(2)+'"/>';
    s += '</svg>';
    return 'url("data:image/svg+xml;charset=utf-8,'+encodeURIComponent(s)+'")';
  }

  var artCache = {};
  function addArt(host){
    var name = host.getAttribute('data-art'), seed = +host.getAttribute('data-seed') || 1;
    var key = name+seed;
    var url = artCache[key] || (artCache[key] = paint(name, seed));
    var clip = document.createElement('div'), el = document.createElement('div');
    clip.className = 'art-clip'; el.className = 'art';
    el.style.backgroundImage = url;
    clip.appendChild(el);
    host.insertBefore(clip, host.firstChild);
    var img = new Image();
    img.onload = function(){ el.classList.add('ready'); };
    img.src = url.slice(5, -2);
    setTimeout(function(){ el.classList.add('ready'); }, 1500);
  }
  // Paint lazily as frames approach the viewport.
  var artIO = new IntersectionObserver(function(es){
    es.forEach(function(e){ if(e.isIntersecting){ artIO.unobserve(e.target); addArt(e.target); } });
  }, {rootMargin:'600px 0px'});
  $$('[data-art]').forEach(function(h){ artIO.observe(h); });

  /* ================= stage scaling ================= */
  function fit(wrap){
    var compact = wrap.clientWidth < 620 && wrap.hasAttribute('data-cw');
    var W = +(compact ? wrap.getAttribute('data-cw') : wrap.getAttribute('data-w'));
    var H = +(compact ? wrap.getAttribute('data-ch') : wrap.getAttribute('data-h'));
    wrap.classList.toggle('compact', compact);
    wrap.style.aspectRatio = W + ' / ' + H;
    wrap.style.setProperty('--s', (wrap.clientWidth / W).toFixed(4));
  }
  var ro = new ResizeObserver(function(es){ es.forEach(function(e){ fit(e.target); }); });
  $$('.stage-wrap').forEach(function(w){ fit(w); ro.observe(w); });
  function scaleOf(stage){ return parseFloat(getComputedStyle(stage.parentElement).getPropertyValue('--s')) || 1; }
  // A source preview opens just under its citation, as the product's popover does, kept inside the stage.
  function anchor(pop, cite, stage){
    var s = scaleOf(stage), sr = stage.getBoundingClientRect(), cr = cite.getBoundingClientRect();
    var x = (cr.left - sr.left)/s - 20, y = (cr.bottom - sr.top)/s + 8;
    x = Math.max(12, Math.min(x, stage.offsetWidth - pop.offsetWidth - 12));
    pop.style.left = x.toFixed(0) + 'px'; pop.style.top = y.toFixed(0) + 'px';
  }

  /* ================= reveal ================= */
  var rvIO = new IntersectionObserver(function(es){
    es.forEach(function(e){ if(e.isIntersecting){ e.target.classList.add('in'); rvIO.unobserve(e.target); } });
  }, {threshold:.12, rootMargin:'0px 0px -40px 0px'});
  $$('.rv').forEach(function(el){ rvIO.observe(el); });

  /* ================= visibility-driven players ================= */
  // Each player is an async loop that runs only while its stage is on screen.
  // With opts.reset, every pass starts from the stage's reset state, and no reset is
  // ever seen: once a pass has changed the stage, the parts in opts.fade (or the
  // whole view) fade out, the stage resets with transitions off, and they fade back in.
  var FADE = 700;
  function player(stage, run, opts){
    opts = opts || {};
    var token = 0, inView = false, running = false, changed = false;
    function want(){ return inView && (!opts.gate || opts.gate()); }
    function resetUnseen(){ stage.classList.add('notrans'); opts.reset(); stage.getBoundingClientRect(); stage.classList.remove('notrans'); }
    async function restart(){
      if(!changed){ resetUnseen(); return; }
      var parts = opts.fade ? $$(opts.fade, stage) : [].slice.call(stage.children);
      var outs = parts.map(function(p){ return p.animate({opacity:[getComputedStyle(p).opacity, 0]}, {duration:FADE, easing:'ease', fill:'forwards'}); });
      await Promise.all(outs.map(function(a){ return a.finished; }));
      resetUnseen();
      outs.forEach(function(a, k){ a.cancel(); parts[k].animate({opacity:[0, getComputedStyle(parts[k]).opacity]}, {duration:FADE, easing:'ease'}); });
      await wait(FADE);
    }
    function refresh(){
      if(want() && !running){
        running = true;
        var my = ++token;
        var alive = function(){ return my === token && want(); };
        (async function(){
          do {
            if(opts.reset){ await restart(); changed = false; if(!alive()) break; }
            changed = true; await run(alive); if(!alive()) break; await wait(opts.hold || 0);
          }
          while(opts.loop && alive());
          if(my === token) running = false;
        })();
      } else if(!want() && running){ token++; running = false; }
    }
    // A stage you scroll to starts once a third of it shows, so its opening is seen.
    // A stage on the first screen plays whenever any of it shows: at the top of a
    // short window it may never reach a third, and it is the first thing people see.
    new IntersectionObserver(function(es){
      var e = es[0], firstScreen = stage.getBoundingClientRect().top + scrollY < innerHeight;
      if(e.intersectionRatio >= .35 || (firstScreen && e.isIntersecting)) inView = true;
      else if(!e.isIntersecting) inView = false;
      refresh();
    }, {threshold:[0,.35]}).observe(stage);
    return {refresh: refresh};
  }
  // Feature stages only animate while their item is the active one.
  var featPlayers = [];
  function featGate(stage){ var f = stage.closest('.fstage'); return function(){ return !f || f.classList.contains('on'); }; }
  function featPlayer(stage, run, opts){ opts.gate = featGate(stage); var p = player(stage, run, opts); featPlayers.push(p); return p; }

  /* ---------- conversation: question, thinking, answer with sources (hero and Ask Coda) ---------- */
  function convo(root){
    var me = $('.a-msg.me', root), think = $('.a-think', root), coda = $('.a-msg.coda', root), cc = $$('.a-cite', root);
    return {
      final: function(){ me.classList.add('on'); coda.classList.add('on'); },
      reset: function(){ [me, think, coda].forEach(function(e){ e.classList.remove('on'); }); cc.forEach(function(c){ c.classList.remove('hot'); }); },
      run: async function(alive, now){
        await wait(now ? 0 : 500); me.classList.add('on'); await wait(800); if(!alive()) return;
        think.classList.add('on'); await wait(2600); if(!alive()) return;
        think.classList.remove('on'); coda.classList.add('on'); await wait(700);
        for(var i=0;i<cc.length;i++){ if(!alive()) return; cc[i].classList.add('hot'); await wait(550); cc[i].classList.remove('hot'); }
      }
    };
  }

  /* ---------- sticky lists: scroll picks the active item ---------- */
  var narrow = matchMedia('(max-width: 900px)');
  function stickyList(root, onChange){
    var items = $$('.fitem', root), active = 0, timer = null, picked = false, visible = false;
    var frame = $('.fframe', root);
    new ResizeObserver(function(){ root.style.setProperty('--fh', frame.offsetHeight + 'px'); }).observe(frame);
    function set(i, force){
      if(i === active && !force) return;
      active = i;
      items.forEach(function(el,k){ el.classList.toggle('on', k===i); el.setAttribute('aria-pressed', String(k===i)); });
      onChange(i);
    }
    function byScroll(){
      if(narrow.matches) return;
      var r0 = root.getBoundingClientRect();
      if(r0.bottom < 0 || r0.top > innerHeight) return;
      var mid = innerHeight/2, best = 0, bestD = 1e9;
      items.forEach(function(el,k){ var r = el.getBoundingClientRect(), d = Math.abs(r.top + r.height/2 - mid); if(d < bestD){ bestD = d; best = k; } });
      set(best);
    }
    var queued = false;
    addEventListener('scroll', function(){ if(queued) return; queued = true; requestAnimationFrame(function(){ queued = false; byScroll(); }); }, {passive:true});
    addEventListener('resize', byScroll);
    // Phones: the frame sits above the list and advances on its own until someone taps.
    function autoNext(){
      clearTimeout(timer);
      if(!narrow.matches || picked || reduced || !visible) return;
      timer = setTimeout(function(){ set((active+1) % items.length); autoNext(); }, 7000);
    }
    new IntersectionObserver(function(es){ visible = es[0].isIntersecting; autoNext(); }, {threshold:.4}).observe($('.fframe', root));
    items.forEach(function(el,k){
      el.addEventListener('click', function(){
        if(narrow.matches){ picked = true; clearTimeout(timer); set(k); }
        else { var r = el.getBoundingClientRect(); scrollTo({top: scrollY + r.top + r.height/2 - innerHeight/2, behavior: reduced ? 'auto' : 'smooth'}); }
      });
    });
    narrow.addEventListener('change', function(){ byScroll(); autoNext(); });
    set(0, true);
    byScroll();
  }

  /* ---------- live work lines ----------
     A running review shows only the step its tools are on now, from the product's fixed list. */
  if(!reduced){
    // Each line keeps its own rhythm, so neighbouring reviews never step in lockstep.
    $$('[data-steps]').forEach(function(el, k){
      var steps = el.getAttribute('data-steps').split('|'), at = 0;
      setTimeout(function(){
        setInterval(function(){
          var r = el.getBoundingClientRect();
          if(r.bottom < 0 || r.top > innerHeight) return;
          el.classList.add('swap');
          setTimeout(function(){ at = (at + 1) % steps.length; el.textContent = steps[at]; el.classList.remove('swap'); }, 260);
        }, 2600 + (k % 3) * 400);
      }, (k % 4) * 900);
    });
  }

  /* ---------- hero: Today, then Maya's flagged change with its sources ---------- */
  section('#heroStage', function(hero){
    var rows = $$('.a-pre .a-row', hero), maya = $('[data-k="maya"]', hero), cites = $$('.hero-reader .a-cite', hero);
    var disc = $('.hero-disc', hero), dq = convo(disc), typed = $('.a-typed', disc), send = $('.a-send', disc);
    var s1 = $('.sat-s1', hero), s3 = $('.sat-s3', hero), discuss = $('[data-k="discuss"]', hero);
    var ptr = $('.pointer', hero), px = $('.ptr-x', hero), py = $('.ptr-y', hero);
    // The cursor aims at the centre of the element it acts on, wherever layout puts it.
    function pointAt(el){
      var s = scaleOf(hero), sr = hero.getBoundingClientRect(), r = el.getBoundingClientRect();
      var x = (r.left - sr.left + r.width/2)/s, y = (r.top - sr.top + r.height/2)/s;
      px.style.transform = 'translateX('+x.toFixed(0)+'px)';
      py.style.transform = 'translateY('+y.toFixed(0)+'px)';
    }
    function hot(i){ cites.forEach(function(c,k){ c.classList.toggle('hot', k===i); }); }
    function heroReset(){
      hero.classList.remove('s-read','drawn','s-disc'); dq.reset(); typed.textContent = typed.getAttribute('data-ph'); typed.classList.remove('typing'); send.classList.remove('ready');
      rows.forEach(function(r){ r.classList.remove('in','sel'); });
      hot(-1); s1.classList.remove('on'); s3.classList.remove('on'); discuss.classList.remove('hot');
      ptr.classList.remove('show','click');
      px.style.transform = ''; py.style.transform = '';
    }
    function heroFinal(){
      rows.forEach(function(r){ r.classList.add('in'); }); maya.classList.add('sel');
      hero.classList.add('s-read','drawn'); hot(2); anchor(s3, cites[2], hero); s3.classList.add('on');
    }
    if(reduced) heroFinal();
    else player(hero, async function(alive){
      await wait(400);
      for(var i=0;i<rows.length;i++){ rows[i].classList.add('in'); await wait(140); }
      await wait(900); if(!alive()) return;
      ptr.classList.add('show'); pointAt($('p', maya)); await wait(1100);
      ptr.classList.add('click'); maya.classList.add('sel'); await wait(350); ptr.classList.remove('click');
      hero.classList.add('s-read'); await wait(600);
      hero.classList.add('drawn'); await wait(700); if(!alive()) return;
      pointAt(cites[0]); await wait(900);
      hot(0); anchor(s1, cites[0], hero); s1.classList.add('on'); await wait(1500); if(!alive()) return;
      pointAt(cites[2]); await wait(900);
      hot(2); s1.classList.remove('on'); anchor(s3, cites[2], hero); s3.classList.add('on'); await wait(1500); if(!alive()) return;
      hot(-1); s3.classList.remove('on'); pointAt(discuss); await wait(1000);
      ptr.classList.add('click'); discuss.classList.add('hot'); await wait(300); ptr.classList.remove('click');
      hero.classList.add('s-disc'); await wait(900); if(!alive()) return;
      pointAt(typed); await wait(800);
      ptr.classList.add('click'); await wait(250); ptr.classList.remove('click'); ptr.classList.remove('show');
      var q = $('.a-msg.me', disc).textContent; typed.classList.add('typing'); typed.textContent = '';
      for(var t=1;t<=q.length;t++){ typed.textContent = q.slice(0,t); await wait(34 + (q.charCodeAt(t-1)===32 ? 40 : 0)); }
      send.classList.add('ready'); await wait(450); if(!alive()) return;
      typed.textContent = typed.getAttribute('data-ph'); typed.classList.remove('typing'); send.classList.remove('ready');
      await dq.run(alive, true); if(!alive()) return;
      await wait(3200);
    }, {loop:true, reset:heroReset});
  });

  /* ---------- 01 Today: one concern is done, the rest of the day stays in Updates ---------- */
  section('.f-today', function(ft){
    var mary = $('[data-k="mary"]', ft), doneBtn = $('[data-k="done"]', ft), n = $('.a-n', ft), also = $('.a-also', ft), ups = $('.a-updates', ft);
    function ftReset(){ mary.classList.remove('gone'); doneBtn.classList.remove('hot','press'); n.textContent = '3'; also.classList.remove('on'); ups.classList.remove('on'); }
    if(reduced){ also.classList.add('on'); ups.classList.add('on'); }
    else featPlayer(ft, async function(alive){
      await wait(1800); if(!alive()) return;
      doneBtn.classList.add('hot'); await wait(500); doneBtn.classList.add('press'); await wait(180); doneBtn.classList.remove('press');
      await wait(300); mary.classList.add('gone'); n.textContent = '2'; await wait(1100); if(!alive()) return;
      also.classList.add('on'); ups.classList.add('on'); await wait(5200);
    }, {loop:true, reset:ftReset, fade:'.a-list, .a-n'});
  });

  /* ---------- 02 Briefing: each line arrives with its source ---------- */
  section('.f-brief', function(fb){
    var lis = $$('.a-lines li', fb), bc = $$('.a-cite', fb), pop = $('.a-pop', fb);
    function fbReset(){ lis.forEach(function(l){ l.classList.remove('on'); }); bc.forEach(function(c){ c.classList.remove('hot'); }); pop.classList.remove('on'); }
    function fbFinal(){ lis.forEach(function(l){ l.classList.add('on'); }); bc[1].classList.add('hot'); anchor(pop, bc[1], fb); pop.classList.add('on'); }
    if(reduced) fbFinal();
    else featPlayer(fb, async function(alive){
      await wait(600);
      for(var i=0;i<lis.length;i++){ if(!alive()) return; lis[i].classList.add('on'); await wait(550); }
      await wait(500); bc[1].classList.add('hot'); anchor(pop, bc[1], fb); await wait(350); pop.classList.add('on');
      await wait(5600);
    }, {loop:true, reset:fbReset, fade:'.a-lines, .a-pop'});
  });

  /* ---------- 03 Ask Coda: the answer comes from the chart, with sources ---------- */
  section('.f-ask', function(fq){
    var cq = convo(fq);
    if(reduced) cq.final();
    else featPlayer(fq, async function(alive){ await cq.run(alive); await wait(5200); }, {loop:true, reset:cq.reset, fade:'.a-msg, .a-think'});
  });

  /* ---------- 04 Follow-ups: Coda keeps checking, then the result is back on Today ---------- */
  section('.f-follow', function(ff){
    var arrive = $('.a-arrive', ff);
    if(reduced) arrive.classList.add('on');
    else featPlayer(ff, async function(alive){
      await wait(5400); if(!alive()) return;
      arrive.classList.add('on'); await wait(5600);
    }, {loop:true, reset:function(){ arrive.classList.remove('on'); }, fade:'.a-arrive'});
  });

  /* ---------- trait tiles ---------- */
  section('.t-src', function(ts){
    var tc = $('.a-cite', ts), tp = $('.a-pop', ts);
    if(reduced){ tc.classList.add('hot'); anchor(tp, tc, ts); tp.classList.add('on'); }
    else player(ts, async function(alive){
      await wait(1400); if(!alive()) return;
      tc.classList.add('hot'); anchor(tp, tc, ts); await wait(300); tp.classList.add('on'); await wait(4200);
    }, {loop:true, reset:function(){ tc.classList.remove('hot'); tp.classList.remove('on'); }, fade:'.a-body, .a-pop'});
  });
  section('.t-you', function(ty){
    if(reduced) return;
    var tb = $$('.a-btn', ty), tk = 0;
    player(ty, async function(alive){
      tb.forEach(function(b){ b.classList.remove('hot'); });
      tb[tk % tb.length].classList.add('hot'); tk++; await wait(1600);
    }, {loop:true});
  });


  section('#physicians .feats', function(physFeats){
    var fstages = $$('.fstage', physFeats);
    stickyList(physFeats, function(i){
      fstages.forEach(function(el,k){ el.classList.toggle('on', k===i); });
      featPlayers.forEach(function(p){ p.refresh(); });
    });
  });

  /* ---------- patient memory: the summary as a table with time ----------
     Each line shows what came before, the weeks since August, and what is true now. The history fills first,
     then a playhead crosses the recent weeks and each line's current value changes as its record arrives. */
  section('#memStage', function(ms){
    var card = $('.mt-card', ms), mtRows = $$('.mt-row', ms), wEl = $('[data-w]', ms), reach = $('.mt-reach', ms);
    var lpill = $('.mt-lpill', ms), hpill = $('.mt-hpill', ms);
    var MON = ['Jan','Feb','Mar','Apr','May','Jun','Jul','Aug','Sep','Oct','Nov','Dec'];
    function D(s){ var p = s.split('-'); return Date.UTC(+p[0], +p[1]-1, +p[2]) / 864e5; }
    function fmt(d, long){ var t = new Date(Math.round(d)*864e5); return long ? MON[t.getUTCMonth()]+' '+t.getUTCFullYear() : MON[t.getUTCMonth()]+' '+t.getUTCDate(); }
    var H0 = D('2019-01-01'), F0 = D('2026-08-01'), F1 = D('2026-09-15');
    // Every mark is placed from its dates (as --x, and --w for bars and bands): the history lane
    // runs H0 to F0, the recent lane F0 to F1.
    // A bar or band without a start or an end runs from the lane's start or to its end.
    function place(lane, a, b){
      function at(el, attr, end){ var v = el.getAttribute(attr); return v ? (D(v) - a)/(b - a)*100 : end; }
      $$(lane+' .mt-in > [data-d]:not(.mt-cap)', ms).forEach(function(el){ el.style.setProperty('--x', at(el, 'data-d').toFixed(2)+'%'); });
      $$(lane+' .mt-bar, '+lane+' .mt-span', ms).forEach(function(el){
        var x = at(el, 'data-from', 0), y = at(el, 'data-to', 100);
        el.style.setProperty('--x', x.toFixed(2)+'%'); el.style.setProperty('--w', (y - x).toFixed(2)+'%');
      });
    }
    place('.mt-h', H0, F0); place('.mt-l', F0, F1);
    function items(sel){ return $$(sel+' [data-d]', ms).map(function(el){ return {el:el, d:D(el.getAttribute('data-d')), on:null}; }); }
    var hItems = items('.mt-row .mt-h'), lItems = items('.mt-row .mt-l');
    var hClips = $$('.mt-row .mt-h .mt-clip', ms), lClips = $$('.mt-row .mt-l .mt-clip', ms);
    var R = mtRows.map(function(r){
      var v = $('.mt-v', r), s = $('.mt-s', r);
      // A line changes when its latest recent record arrives.
      var ev = Math.max.apply(null, $$('.mt-l [data-d], .mt-l [data-from]', r).map(function(el){ return D(el.getAttribute('data-d') || el.getAttribute('data-from')); }));
      return {el:r, v:v, s:s, now:v.innerHTML, ic:s.innerHTML, ev:ev, bat:D(r.getAttribute('data-bat')), base:r.getAttribute('data-base'), state:''};
    });
    var IC0 = {};
    $$('.mt-row', ms).forEach(function(r){ IC0[r.getAttribute('data-ic')] = $('.mt-s', r).innerHTML; });
    var BIC = {note:IC0.note, rx:IC0.rx, msg:IC0.msg, lab:IC0.lab, pulse:IC0.pulse,
      clock:'<svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="8.5"/><path d="M12 7.5V12l3 2"/></svg>'};
    function sweep(list, clips, d, pct){
      list.forEach(function(it){ var on = it.d <= d + .5; if(on !== it.on){ it.el.classList.toggle('on', on); it.on = on; } });
      var cl = 'inset(0 '+(100 - pct).toFixed(2)+'% 0 0)'; clips.forEach(function(c){ c.style.clipPath = cl; });
    }
    function setH(d){ var p = Math.max(0, Math.min(100, (d - H0)/(F0 - H0)*100)); card.style.setProperty('--hp', p.toFixed(2)); sweep(hItems, hClips, d, p); hpill.textContent = new Date(Math.round(d)*864e5).getUTCFullYear(); }
    var tks = $$('.mt-tk', ms).map(function(t){ return {el:t, at:t.hasAttribute('data-d') ? parseFloat(t.style.getPropertyValue('--x')) : 100}; });
    function setP(d){
      var p = Math.max(0, Math.min(100, (d - F0)/(F1 - F0)*100)); card.style.setProperty('--p', p.toFixed(2)); sweep(lItems, lClips, d, p); lpill.textContent = d >= F1 - .5 ? 'Today' : fmt(d);
      var live = card.classList.contains('l-live'); tks.forEach(function(t){ t.el.style.opacity = live && Math.abs(p - t.at) < 11 ? 0 : 1; });
    }
    function show(r, html, icon, flash){
      if(r.state === html) return; r.state = html;
      r.v.innerHTML = html; r.v.classList.remove('sk'); r.s.innerHTML = icon; r.s.classList.remove('off');
      r.v.animate([{opacity:.1}, {opacity:1}], {duration:flash ? 460 : 280, easing:'ease-out'});
      if(flash){ r.el.classList.add('upd'); r.s.animate([{transform:'scale(.4)'}, {transform:'scale(1.15)', offset:.6}, {transform:'none'}], {duration:420, easing:'cubic-bezier(.2,.8,.2,1)'}); }
    }
    function blank(r){ r.state = ''; r.v.classList.add('sk'); r.s.classList.add('off'); r.el.classList.remove('upd'); }
    function tween(dur, fn){
      return new Promise(function(res){
        var t0 = performance.now();
        (function step(now){ var t = Math.min(1, (now - t0)/dur); fn(t < .5 ? 4*t*t*t : 1 - Math.pow(-2*t + 2, 3)/2); if(t < 1) requestAnimationFrame(step); else res(); })(t0);
      });
    }
    function stageXY(el){ var s = scaleOf(ms), sr = card.getBoundingClientRect(), r = el.getBoundingClientRect(); return {x:(r.left + r.width/2 - sr.left)/s, y:(r.top + r.height/2 - sr.top)/s}; }
    var k24 = $('.k24', ms);
    function memReset(){
      reach.getAnimations().forEach(function(a){ a.cancel(); });
      card.classList.remove('l-live','h-live'); k24.classList.remove('glow');
      R.forEach(blank); wEl.textContent = '';
      hItems.concat(lItems).forEach(function(it){ it.on = null; });
      setH(H0); setP(F0);
    }
    function memFinal(){ setH(F0); setP(F1); R.forEach(function(r){ show(r, r.now, r.ic, false); }); k24.classList.add('glow'); wEl.textContent = fmt(F1); }

    if(reduced) memFinal();
    else player(ms, async function(alive){
      await wait(500); if(!alive()) return;
      // what came before: years pass, and each line shows what was true as they do
      card.classList.add('h-live');
      await tween(2600, function(t){
        var d = H0 + (F0 - H0)*t; setH(d);
        R.forEach(function(r){ if(d >= r.bat) show(r, r.base, BIC[r.el.getAttribute('data-bic')], false); });
        var latest = R.reduce(function(m, r){ return d >= r.bat && r.bat > m ? r.bat : m; }, 0); if(latest) wEl.textContent = fmt(latest, true);
      });
      card.classList.remove('h-live'); await wait(400); if(!alive()) return;
      // the weeks since August: a playhead crosses them, and each line changes as its record arrives
      card.classList.add('l-live');
      var evs = R.map(function(r){ return r.ev; }).filter(function(v, i, a){ return a.indexOf(v) === i; }).sort(function(a, b){ return a - b; });
      var P = F0;
      for(var i=0;i<evs.length;i++){
        if(!alive()) return;
        var to = evs[i], from = P;
        await tween(Math.max(650, (to - from)*42), function(t){ P = from + (to - from)*t; setP(P); });
        var hit = R.filter(function(r){ return r.ev === to; });
        hit.forEach(function(r){ if(!r.el.hasAttribute('data-reach')) show(r, r.now, r.ic, true); });
        wEl.textContent = fmt(to);
        var back = hit.filter(function(r){ return r.el.hasAttribute('data-reach'); })[0];
        if(back){
          await wait(600); if(!alive()) return;
          // a new symptom brings the 2024 reaction on the same line forward
          var a = stageXY(k24), b = stageXY($('.mt-l .mt-dot', back.el));
          k24.classList.add('glow');
          reach.style.left = a.x.toFixed(1)+'px'; reach.style.top = a.y.toFixed(1)+'px'; reach.style.width = (b.x - a.x).toFixed(1)+'px';
          await reach.animate([{transform:'scaleX(0)', opacity:1}, {transform:'scaleX(1)', opacity:1}], {duration:800, easing:'cubic-bezier(.65,0,.35,1)', fill:'forwards'}).finished;
          show(back, back.now, back.ic, true);
          reach.animate([{transform:'scaleX(1)', opacity:1}, {transform:'scaleX(1)', opacity:0}], {duration:900, delay:600, fill:'forwards'});
        }
        await wait(900);
        hit.forEach(function(r){ r.el.classList.remove('upd'); });
      }
      var from2 = P; await tween(700, function(t){ setP(from2 + (F1 - from2)*t); });
      await wait(5200);
      // Between passes the changing parts fade; the row names and the frame stay.
    }, {loop:true, reset:memReset, fade:'.mt-row .mt-in, .mt-row .mt-v, .mt-row .mt-s, .mt-pill, .mt-tk, .mt-w b'});
  });

  /* ---------- duet ---------- */
  section('.duet-stage', function(ds){
    var cms = $$('.cm', ds), att = $('.ph-attach', ds), stat = $('.ph-stat', ds), pss = $$('.ps', ds);
    function dsFinal(){ cms.concat(pss, [att, stat]).forEach(function(e){ e.classList.add('on'); }); }
    if(reduced) dsFinal();
    else player(ds, async function(alive){
      await wait(500);
      stat.classList.add('on');
      pss[2].classList.add('on'); pss[3].classList.add('on');
      for(var i=0;i<cms.length;i++){
        if(!alive()) return;
        cms[i].classList.add('on');
        if(i===2){ pss[0].classList.add('on'); pss[1].classList.add('on'); }
        await wait(1100);
      }
      att.classList.add('on'); await wait(7000);
    }, {loop:true, reset:function(){ cms.concat(pss, [att, stat]).forEach(function(e){ e.classList.remove('on'); }); }, fade:'.cm, .ps, .ph-attach, .ph-stat'});
  });
})();
