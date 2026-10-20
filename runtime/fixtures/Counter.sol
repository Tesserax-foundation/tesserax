// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

/// ML-DSA-87 verification precompile of the Tesserax runtime.
interface IMlDsa87 {
    function verify(
        bytes calldata publicKey,
        bytes calldata context,
        bytes calldata message,
        bytes calldata signature
    ) external view returns (bool);
}

/// Test contract used by the runtime tests.
contract Counter {
    IMlDsa87 constant ML_DSA_87 = IMlDsa87(address(uint160(0xD5A00000)));

    uint256 public count;

    function increment() external {
        count += 1;
    }

    function verifyMlDsa(
        bytes calldata publicKey,
        bytes calldata context,
        bytes calldata message,
        bytes calldata signature
    ) external view returns (bool) {
        return ML_DSA_87.verify(publicKey, context, message, signature);
    }

    function recover(bytes32 hash, uint8 v, bytes32 r, bytes32 s) external pure returns (address) {
        return ecrecover(hash, v, r, s);
    }
}
