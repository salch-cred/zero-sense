.PHONY: help setup contracts-build contracts-deploy api-start sim-run frontend-open clean

help:
	@echo ""
	@echo "  🤖 ZeroSense Build Commands"
	@echo "  ══════════════════════════"
	@echo "  make setup           Install all dependencies"
	@echo "  make contracts-build Build all Soroban contracts"
	@echo "  make contracts-deploy Deploy to Stellar testnet"
	@echo "  make proof-test      Generate a test ZK proof"
	@echo "  make api-start       Start FastAPI backend"
	@echo "  make sim-run         Run robot simulation"
	@echo "  make frontend-open   Open dashboard in browser"
	@echo "  make guardian-start  Start all 7 Guardian agents"
	@echo "  make demo            Run full demo (api + sim + frontend)"
	@echo ""

setup:
	@echo "📦 Installing Python dependencies..."
	pip install -r requirements.txt
	@echo "🦀 Setting up Rust + RISC Zero..."
	curl -L https://risczero.com/install | bash
	rzup install
	@echo "⭐ Setting up Stellar CLI..."
	cargo install stellar-cli
	@echo "✅ Setup complete!"

contracts-build:
	@echo "🔨 Building Soroban contracts..."
	@for contract in verifier payment reputation insurance; do \
		echo "  Building $$contract..."; \
		cd contracts/$$contract && cargo build --target wasm32v1-none --release; \
		cd ../..; \
	done
	@echo "✅ Contracts built!"

contracts-deploy:
	@echo "🚀 Deploying to Stellar testnet..."
	@stellar keys fund zerosense-dev --network testnet
	@for contract in verifier payment reputation insurance; do \
		echo "  Deploying $$contract..."; \
		stellar contract deploy \
			--wasm contracts/$$contract/target/wasm32v1-none/release/zerosense_$$contract.wasm \
			--source zerosense-dev \
			--network testnet; \
	done
	@echo "✅ Contracts deployed! Update .env with contract IDs."

proof-test:
	@echo "🔐 Generating test ZK proof..."
	cd zkvm && cargo run --release --bin host

api-start:
	@echo "🌐 Starting ZeroSense FastAPI backend..."
	uvicorn api.main:app --reload --port 8000

sim-run:
	@echo "🤖 Running robot simulation..."
	python simulation/robot_sim.py

frontend-open:
	@echo "🖥️  Opening dashboard..."
	open frontend/index.html || xdg-open frontend/index.html

guardian-start:
	@echo "🤖 Starting Guardian v2 (7 autonomous agents)..."
	curl -X POST http://localhost:8000/guardian/start

demo:
	@echo "🎬 Running full ZeroSense demo..."
	@make api-start &
	@sleep 3
	@make sim-run

clean:
	find . -name "target" -type d -exec rm -rf {} + 2>/dev/null; true
	find . -name "__pycache__" -type d -exec rm -rf {} + 2>/dev/null; true
	find . -name "*.pyc" -delete 2>/dev/null; true
