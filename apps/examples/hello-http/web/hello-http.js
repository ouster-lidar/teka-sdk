(function () {
    'use strict';

    var link = document.getElementById('hello-http-link');
    if (!link) return;

    var hostname = window.location.hostname || 'localhost';
    if (hostname.indexOf(':') >= 0 && hostname.charAt(0) !== '[') {
        hostname = '[' + hostname + ']';
    }
    var url = 'http://' + hostname + ':__HELLO_HTTP_PORT__/';
    link.href = url;
    link.textContent = url;
})();
