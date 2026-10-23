// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

/// A minimal token, the example of docs/contracts.md and the contract the SDK tests deploy.
contract Token {
    string public name;
    uint256 public totalSupply;
    mapping(address => uint256) public balanceOf;

    event Transfer(address indexed from, address indexed to, uint256 value);

    error InsufficientBalance(uint256 available, uint256 required);

    constructor(string memory name_, uint256 supply) {
        name = name_;
        totalSupply = supply;
        balanceOf[msg.sender] = supply;
        emit Transfer(address(0), msg.sender, supply);
    }

    function transfer(address to, uint256 value) external returns (bool) {
        uint256 available = balanceOf[msg.sender];
        if (available < value) revert InsufficientBalance(available, value);
        balanceOf[msg.sender] = available - value;
        balanceOf[to] += value;
        emit Transfer(msg.sender, to, value);
        return true;
    }

    function burn(uint256 value) external {
        require(balanceOf[msg.sender] >= value, "burn exceeds balance");
        balanceOf[msg.sender] -= value;
        totalSupply -= value;
        emit Transfer(msg.sender, address(0), value);
    }
}
