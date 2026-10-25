// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.28;

import {TesseraxSwap} from "../src/TesseraxSwap.sol";

/// The cheatcodes of Foundry used here.
interface Vm {
    function warp(uint256 timestamp) external;
    function deal(address who, uint256 amount) external;
    function prank(address sender) external;
    function expectRevert(bytes4 selector) external;
}

/// A token returning true, as USDC.
contract Token {
    mapping(address => uint256) public balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;

    function mint(address to, uint256 amount) external {
        balanceOf[to] += amount;
    }

    function approve(address spender, uint256 amount) external returns (bool) {
        allowance[msg.sender][spender] = amount;
        return true;
    }

    function transfer(address to, uint256 amount) public virtual returns (bool) {
        balanceOf[msg.sender] -= amount;
        balanceOf[to] += amount;
        return true;
    }

    function transferFrom(address from, address to, uint256 amount) public virtual returns (bool) {
        allowance[from][msg.sender] -= amount;
        balanceOf[from] -= amount;
        balanceOf[to] += amount;
        return true;
    }
}

/// A token returning nothing, as USDT on Ethereum.
contract NoReturnToken {
    mapping(address => uint256) public balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;

    function mint(address to, uint256 amount) external {
        balanceOf[to] += amount;
    }

    function approve(address spender, uint256 amount) external {
        allowance[msg.sender][spender] = amount;
    }

    function transfer(address to, uint256 amount) external {
        balanceOf[msg.sender] -= amount;
        balanceOf[to] += amount;
    }

    function transferFrom(address from, address to, uint256 amount) external {
        allowance[from][msg.sender] -= amount;
        balanceOf[from] -= amount;
        balanceOf[to] += amount;
    }
}

/// A token keeping 1% of each transfer.
contract FeeToken is Token {
    function transferFrom(address from, address to, uint256 amount) public override returns (bool) {
        allowance[from][msg.sender] -= amount;
        balanceOf[from] -= amount;
        balanceOf[to] += amount - amount / 100;
        return true;
    }
}

contract TesseraxSwapTest {
    Vm constant vm = Vm(address(uint160(uint256(keccak256("hevm cheat code")))));
    TesseraxSwap swap;

    address constant TAKER = address(0x7A4E);
    address constant MAKER = address(0x3A4E);
    address constant RELAYER = address(0x4E1A);
    bytes32 constant SECRET = bytes32(uint256(0x5ec4e7));
    bytes32 constant ACCOUNT = bytes32(uint256(0xacc));
    uint64 constant OFFER = 7;

    function setUp() public {
        swap = new TesseraxSwap();
        HASH = sha256(abi.encodePacked(SECRET));
        vm.warp(1_000_000);
    }

    /// The hash of the secret, computed once: a call to the SHA-256 precompile between a prank and
    /// its call would take the prank.
    bytes32 HASH;

    function hash() internal view returns (bytes32) {
        return HASH;
    }

    function lockEth(uint256 amount) internal returns (bytes32) {
        vm.deal(TAKER, amount);
        vm.prank(TAKER);
        return swap.lock{value: amount}(MAKER, address(0), amount, hash(), uint64(block.timestamp + 1 days), OFFER, ACCOUNT);
    }

    function testAnyoneClaimsForTheRecipientWithTheSecret() public {
        bytes32 id = lockEth(1 ether);
        vm.expectRevert(TesseraxSwap.WrongSecret.selector);
        swap.claim(id, bytes32(uint256(1)));
        vm.prank(RELAYER);
        swap.claim(id, SECRET);
        require(MAKER.balance == 1 ether, "the maker is paid");
        require(RELAYER.balance == 0, "the relayer gets nothing");
        vm.expectRevert(TesseraxSwap.NotLocked.selector);
        swap.claim(id, SECRET);
    }

    function testTheSenderTakesItBackAfterTheTimelock() public {
        bytes32 id = lockEth(1 ether);
        vm.expectRevert(TesseraxSwap.NotExpired.selector);
        swap.refund(id);
        vm.warp(block.timestamp + 1 days);
        vm.expectRevert(TesseraxSwap.Expired.selector);
        swap.claim(id, SECRET);
        swap.refund(id);
        require(TAKER.balance == 1 ether, "the taker is refunded");
        vm.expectRevert(TesseraxSwap.NotLocked.selector);
        swap.refund(id);
    }

    function testLocksAreChecked() public {
        vm.deal(TAKER, 2 ether);
        vm.prank(TAKER);
        vm.expectRevert(TesseraxSwap.WrongValue.selector);
        swap.lock{value: 1 ether}(MAKER, address(0), 2 ether, hash(), uint64(block.timestamp + 1 days), OFFER, ACCOUNT);
        vm.prank(TAKER);
        vm.expectRevert(TesseraxSwap.InvalidLock.selector);
        swap.lock{value: 1 ether}(MAKER, address(0), 1 ether, hash(), uint64(block.timestamp), OFFER, ACCOUNT);
        vm.prank(TAKER);
        vm.expectRevert(TesseraxSwap.InvalidLock.selector);
        swap.lock{value: 1 ether}(MAKER, address(0), 1 ether, hash(), uint64(block.timestamp + 1 days), OFFER, bytes32(0));
    }

    function testTokensAsUsdcAndUsdt() public {
        Token usdc = new Token();
        NoReturnToken usdt = new NoReturnToken();
        usdc.mint(TAKER, 500e6);
        usdt.mint(TAKER, 500e6);
        vm.prank(TAKER);
        usdc.approve(address(swap), 500e6);
        vm.prank(TAKER);
        usdt.approve(address(swap), 500e6);

        vm.prank(TAKER);
        bytes32 a = swap.lock(MAKER, address(usdc), 500e6, hash(), uint64(block.timestamp + 1 days), OFFER, ACCOUNT);
        vm.prank(TAKER);
        bytes32 b = swap.lock(MAKER, address(usdt), 500e6, hash(), uint64(block.timestamp + 1 days), OFFER, ACCOUNT);
        require(usdc.balanceOf(address(swap)) == 500e6 && usdt.balanceOf(address(swap)) == 500e6, "locked");
        require(a != b, "every lock has its identifier");

        swap.claim(a, SECRET);
        swap.claim(b, SECRET);
        require(usdc.balanceOf(MAKER) == 500e6 && usdt.balanceOf(MAKER) == 500e6, "the maker is paid");
    }

    function testFeeOnTransferTokensAreRefused() public {
        FeeToken fee = new FeeToken();
        fee.mint(TAKER, 100e6);
        vm.prank(TAKER);
        fee.approve(address(swap), 100e6);
        vm.prank(TAKER);
        vm.expectRevert(TesseraxSwap.FeeOnTransfer.selector);
        swap.lock(MAKER, address(fee), 100e6, hash(), uint64(block.timestamp + 1 days), OFFER, ACCOUNT);
    }
}
