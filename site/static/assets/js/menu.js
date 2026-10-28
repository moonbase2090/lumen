// Mobile nav: keep aria-expanded in sync with <details open>. Menu works without JS.
document.querySelectorAll('details.mnav').forEach(function(d){
  var s=d.querySelector('summary');
  d.addEventListener('toggle',function(){s.setAttribute('aria-expanded',d.open?'true':'false')});
  document.addEventListener('keydown',function(e){if(e.key==='Escape'&&d.open){d.open=false;s.focus()}});
});
