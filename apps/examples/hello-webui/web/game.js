(function () {
    'use strict';

    const canvas = document.getElementById('hw-snake-canvas');
    if (!canvas) return;

    const ctx = canvas.getContext('2d');
    const scoreEl = document.getElementById('hw-snake-score');
    const overlay = document.getElementById('hw-snake-overlay');
    const msgEl = document.getElementById('hw-snake-msg');
    const restartBtn = document.getElementById('hw-snake-restart');

    const GRID = 20;
    let cell = 0;
    const KEYS = {
        arrowup: [0, -1],
        arrowdown: [0, 1],
        arrowleft: [-1, 0],
        arrowright: [1, 0],
    };

    let snake;
    let dir;
    let nextDir;
    let food;
    let score;
    let timer;

    function resizeCanvas() {
        const size = Math.floor(canvas.clientWidth);
        if (size < 1) return;
        canvas.width = size;
        canvas.height = size;
        cell = size / GRID;
        draw();
    }

    function randCell() {
        return { x: Math.floor(Math.random() * GRID), y: Math.floor(Math.random() * GRID) };
    }

    function spawnFood() {
        do {
            food = randCell();
        } while (snake.some((s) => s.x === food.x && s.y === food.y));
    }

    function reset() {
        snake = [{ x: 10, y: 10 }, { x: 9, y: 10 }, { x: 8, y: 10 }];
        dir = nextDir = [1, 0];
        score = 0;
        if (scoreEl) scoreEl.textContent = '0';
        if (overlay) overlay.hidden = true;
        spawnFood();
        draw();
        clearInterval(timer);
        timer = setInterval(tick, 110);
        canvas.focus();
    }

    function endGame() {
        clearInterval(timer);
        if (msgEl) msgEl.textContent = 'Game over';
        if (overlay) overlay.hidden = false;
    }

    function tick() {
        dir = nextDir;
        const head = { x: snake[0].x + dir[0], y: snake[0].y + dir[1] };
        if (head.x < 0 || head.x >= GRID || head.y < 0 || head.y >= GRID
            || snake.some((s) => s.x === head.x && s.y === head.y)) {
            endGame();
            return;
        }
        snake.unshift(head);
        if (head.x === food.x && head.y === food.y) {
            score += 10;
            if (scoreEl) scoreEl.textContent = String(score);
            spawnFood();
        } else {
            snake.pop();
        }
        draw();
    }

    function drawCell(x, y, fill) {
        const pad = 1;
        ctx.fillStyle = fill;
        ctx.fillRect(x * cell + pad, y * cell + pad, cell - pad * 2, cell - pad * 2);
    }

    function draw() {
        ctx.fillStyle = '#0a0a12';
        ctx.fillRect(0, 0, canvas.width, canvas.height);
        drawCell(food.x, food.y, '#fbbf24');
        snake.forEach((seg, i) => drawCell(seg.x, seg.y, i === 0 ? '#4ade80' : '#22c55e'));
    }

    canvas.addEventListener('keydown', (e) => {
        if (!timer) return;
        const d = KEYS[e.key.toLowerCase()];
        if (!d || (d[0] === -dir[0] && d[1] === -dir[1])) return;
        nextDir = d;
        e.preventDefault();
    });
    restartBtn?.addEventListener('click', reset);
    resizeCanvas();
    window.addEventListener('resize', resizeCanvas);
    reset();
})();
