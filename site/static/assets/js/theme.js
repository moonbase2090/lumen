(function(){var d=document.documentElement,k="lumen-theme";
var s=null;try{s=localStorage.getItem(k)}catch(e){}
d.dataset.theme=s||(matchMedia("(prefers-color-scheme: dark)").matches?"dark":"light");
document.addEventListener("DOMContentLoaded",function(){
var b=document.querySelector(".toggle");if(b)b.addEventListener("click",function(){d.dataset.theme=d.dataset.theme==="dark"?"light":"dark";try{localStorage.setItem(k,d.dataset.theme)}catch(e){}});
var q=document.querySelector(".search input");document.addEventListener("keydown",function(e){if(e.key==="/"&&q&&document.activeElement!==q&&!/INPUT|TEXTAREA|SELECT/.test(document.activeElement.tagName)){e.preventDefault();q.focus()}});});})();
